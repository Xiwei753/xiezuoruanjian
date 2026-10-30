//! # AI 请求输入 / 模型响应输出事件
//!
//! 一次 AI 对话的请求输入与模型响应输出是两条独立的事实，不等于一次对象
//! 修改。真实 Agent 会话（#506）在请求发出时记录 [`AiUsageDirection::Input`]
//! 事件，在响应完成时记录 [`AiUsageDirection::Output`] 事件；两条事件用同一
//! `conversation_id` / `turn_id` 关联，但各自拥有真实发生时间、业务日和当地
//! 小时。这样 23:59 发起、00:01 完成的请求不会被错误地算进同一天同一小时。
//!
//! 事件只由实际产生请求/响应的 Agent 适配层创建。`char_count` 是该方向内容
//! 的真实字符数；不从正文修改量反推，也不挂到可能修改多个对象的
//! [`super::contribution::MutationContext`] 上，避免一次操作重复累计输入输出。
//!
//! `event_id` 由调用方为每个方向生成并保持稳定；同一方向重试必须复用同一 ID，
//! 存储层据此幂等去重。

use serde::{Deserialize, Serialize};

use crate::writing_stats::calendar;

/// AI 事件的方向：用户请求输入，或模型响应输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiUsageDirection {
    Input,
    Output,
}

impl AiUsageDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

/// 为同一轮次的某个方向生成稳定事件 ID。
///
/// 请求和响应必须使用不同方向，因此会得到不同 ID；同一方向重试时重复调用
/// 本函数得到相同结果，存储层可据此幂等去重。
pub fn stable_event_id(
    conversation_id: &str,
    turn_id: &str,
    direction: AiUsageDirection,
) -> String {
    format!(
        "ai-usage:{conversation_id}:{turn_id}:{}",
        direction.as_str()
    )
}

/// 一次 AI 请求输入或模型响应输出的来源事实。
///
/// 每个方向单独一条事件；同一次对话轮次的两条事件通过
/// `conversation_id` / `turn_id` 关联，而不是共享同一个时间字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageEvent {
    /// 调用方为**该方向**生成的稳定事件 ID；重放同一条事件不会重复计数。
    pub event_id: String,
    /// 事件来源设备；由 store 写入时补齐，同步来的事件保留原分区。
    #[serde(default)]
    pub origin_device_id: String,
    /// 该方向事件真实发生的时间（请求发出或响应完成）。
    pub occurred_at_ms: i64,
    /// 事件发生设备当时的业务日历日；与事件一起同步，不在查看端重算。
    #[serde(default)]
    pub local_date: String,
    /// 事件发生设备当时的本地小时（0–23）；旧事件或缺失时保持 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hour: Option<u8>,
    /// 发起请求的作品；空串表示不属于单个作品。
    #[serde(default)]
    pub project_id: String,
    /// Agent 原生会话/线程 ID，用于把同一次对话的请求与响应关联起来。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// 同一次对话轮次 ID；与 `conversation_id` 一起关联请求和响应。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// 本事件是请求输入还是模型输出。
    pub direction: AiUsageDirection,
    /// 该方向内容的真实字符数。
    #[serde(default)]
    pub char_count: u32,
}

impl AiUsageEvent {
    /// 记录一次请求输入事件。`event_id` 为空时生成 UUID；跨进程重试时应传入
    /// 由调用方生成的稳定 ID。
    pub fn request(
        event_id: impl Into<String>,
        project_id: impl Into<String>,
        occurred_at_ms: i64,
        input_chars: u32,
    ) -> Self {
        Self::new(
            event_id,
            project_id,
            occurred_at_ms,
            AiUsageDirection::Input,
            input_chars,
        )
    }

    /// 记录一次模型输出事件。`event_id` 为空时生成 UUID；跨进程重试时应传入
    /// 由调用方生成的稳定 ID。
    pub fn response(
        event_id: impl Into<String>,
        project_id: impl Into<String>,
        occurred_at_ms: i64,
        output_chars: u32,
    ) -> Self {
        Self::new(
            event_id,
            project_id,
            occurred_at_ms,
            AiUsageDirection::Output,
            output_chars,
        )
    }

    /// 以指定方向、真实发生时间创建事件。
    pub fn new(
        event_id: impl Into<String>,
        project_id: impl Into<String>,
        occurred_at_ms: i64,
        direction: AiUsageDirection,
        char_count: u32,
    ) -> Self {
        let event_id = event_id.into();
        let event_id = if event_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            event_id
        };
        Self {
            event_id,
            origin_device_id: String::new(),
            occurred_at_ms,
            local_date: calendar::local_date_at(occurred_at_ms).unwrap_or_default(),
            local_hour: calendar::local_hour_at(occurred_at_ms),
            project_id: project_id.into(),
            conversation_id: None,
            turn_id: None,
            direction,
            char_count,
        }
    }

    pub fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    /// 同时设置会话与轮次 ID，便于请求/响应两条事件互相追溯。
    pub fn with_conversation_turn(
        mut self,
        conversation_id: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self.turn_id = Some(turn_id.into());
        self
    }

    /// 事件实际归属的业务日；缺少持久化日期时只按 UTC 兜底，不按查看端时区。
    pub fn business_date(&self) -> String {
        if !self.local_date.is_empty() {
            return self.local_date.clone();
        }
        calendar::utc_date_at(self.occurred_at_ms).unwrap_or_else(|_| "1970-01-01".to_string())
    }
}
