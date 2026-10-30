//! # AI 请求/响应事件
//!
//! 一次 AI 对话请求与模型响应是独立的事实，不等于一次对象修改。真实 Agent
//! 会话（#506）在请求发出、响应完成后记录本事件；统计投影再把它并入既有的
//! 逐日 actor / 小时切片，而不是挂到可能修改多个对象的
//! [`super::contribution::MutationContext`] 上，避免一次操作重复累计输入输出。
//!
//! 事件只由实际产生请求/响应的 Agent 适配层创建。`input_chars` 是用户请求
//! 内容的真实字符数，`output_chars` 是模型产出的真实字符数；二者都不从正文
//! 修改量反推。事件 ID 由调用方提供以保证重试幂等；为空时生成 UUID。

use serde::{Deserialize, Serialize};

use crate::writing_stats::calendar;

/// 一次 AI 请求/响应的来源事实。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsageEvent {
    /// 调用方生成的稳定事件 ID；重放同一条事件不会重复计数。
    pub event_id: String,
    /// 事件来源设备；由 store 写入时补齐，同步来的事件保留原分区。
    #[serde(default)]
    pub origin_device_id: String,
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
    /// Agent 原生会话/线程 ID，仅用于追溯，不参与统计分桶。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// 用户请求内容的真实字符数。
    #[serde(default)]
    pub input_chars: u32,
    /// 模型输出的真实字符数。
    #[serde(default)]
    pub output_chars: u32,
}

impl AiUsageEvent {
    /// 以请求发生时的真实时间创建事件。
    ///
    /// `event_id` 为空时生成 UUID；调用方若需要跨进程重试幂等，应传入稳定 ID。
    pub fn new(
        event_id: impl Into<String>,
        project_id: impl Into<String>,
        occurred_at_ms: i64,
        input_chars: u32,
        output_chars: u32,
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
            input_chars,
            output_chars,
        }
    }

    pub fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
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
