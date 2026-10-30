//! # Agent 会话持久化模型
//!
//! Issue #1016：Linux Qt 的 AI 会话历史由 Core 保存，平台端不另存第二份。
//! 会话记录 Agent 身份、原生 session/thread 身份、每轮用户输入、模型输出
//! 片段与工具事件。正文仍是纯文本；本模块只保存对话事实，不解释任何 Agent
//! 的进程协议。

use serde::{Deserialize, Serialize};

/// 会话状态。运行状态由平台端在真实请求生命周期内更新。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentConversationStatus {
    Idle,
    Running,
    Failed,
    Archived,
}

impl AgentConversationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }
}

/// 一轮对话的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTurnStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// 助手回复中的一个内容片段。工具调用过程单独记录在
/// [`AgentToolEvent`]，避免把可读文本和机器事件混在一起。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentMessagePart {
    Text { text: String },
    Reasoning { text: String },
}

impl AgentMessagePart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning { text: text.into() }
    }

    pub fn as_text(&self) -> &str {
        match self {
            Self::Text { text } | Self::Reasoning { text } => text,
        }
    }
}

/// 一次工具调用的真实生命周期。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolEvent {
    pub tool_call_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<String>,
    /// `running` / `completed` / `failed`
    pub status: String,
    pub started_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<i64>,
}

/// 一轮用户请求与助手回复。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurn {
    pub id: String,
    pub sequence: u32,
    pub user_message: String,
    #[serde(default)]
    pub assistant_parts: Vec<AgentMessagePart>,
    #[serde(default)]
    pub tool_events: Vec<AgentToolEvent>,
    pub status: AgentTurnStatus,
    pub started_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AgentTurn {
    pub fn new(sequence: u32, user_message: String, started_at_ms: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            sequence,
            user_message,
            assistant_parts: Vec::new(),
            tool_events: Vec::new(),
            status: AgentTurnStatus::Running,
            started_at_ms,
            completed_at_ms: None,
            error: None,
        }
    }

    pub fn append_text(&mut self, text: &str) {
        match self.assistant_parts.last_mut() {
            Some(AgentMessagePart::Text { text: existing }) => existing.push_str(text),
            _ => self.assistant_parts.push(AgentMessagePart::Text {
                text: text.to_string(),
            }),
        }
    }

    pub fn append_reasoning(&mut self, text: &str) {
        match self.assistant_parts.last_mut() {
            Some(AgentMessagePart::Reasoning { text: existing }) => existing.push_str(text),
            _ => self.assistant_parts.push(AgentMessagePart::Reasoning {
                text: text.to_string(),
            }),
        }
    }

    /// 助手可见文本的总字符数；用于 AI 使用统计的 Output 事件。
    pub fn assistant_char_count(&self) -> u32 {
        let chars: usize = self
            .assistant_parts
            .iter()
            .map(|part| part.as_text().chars().count())
            .sum();
        u32::try_from(chars).unwrap_or(u32::MAX)
    }

    pub fn complete(&mut self, at_ms: i64) {
        self.status = AgentTurnStatus::Completed;
        self.completed_at_ms = Some(at_ms);
    }

    pub fn fail(&mut self, at_ms: i64, error: impl Into<String>) {
        self.status = AgentTurnStatus::Failed;
        self.completed_at_ms = Some(at_ms);
        self.error = Some(error.into());
    }

    pub fn cancel(&mut self, at_ms: i64) {
        self.status = AgentTurnStatus::Cancelled;
        self.completed_at_ms = Some(at_ms);
    }
}

/// 一个持久化 AI 会话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversation {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub agent_name: String,
    /// 用户选择的真实可执行文件路径；恢复会话时必须继续使用同一安装。
    #[serde(default)]
    pub launch_path: String,
    /// 创建会话时使用的适配器协议（`acp` / `server` / `cli`）。
    #[serde(default)]
    pub agent_protocol: String,
    /// 创建会话时目录声明的启动参数，恢复时必须按同一组参数启动。
    #[serde(default)]
    pub agent_launch_args: Vec<String>,
    /// 适配器映射版本；协议解释变化时用于提示旧会话重新选择。
    #[serde(default)]
    pub agent_adapter_version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: AgentConversationStatus,
    /// Agent 原生会话 / 线程 ID，用于 `--resume` / `thread/resume` /
    /// ACP `session/load`。缺失时下一轮重新创建。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub turns: Vec<AgentTurn>,
}

fn default_schema_version() -> u32 {
    1
}

impl AgentConversation {
    pub fn new(
        project_id: impl Into<String>,
        agent_id: impl Into<String>,
        agent_name: impl Into<String>,
        launch_path: impl Into<String>,
        agent_protocol: impl Into<String>,
        agent_launch_args: Vec<String>,
        agent_adapter_version: u32,
        title: impl Into<String>,
        model: Option<String>,
        now_ms: i64,
    ) -> Self {
        Self {
            schema_version: default_schema_version(),
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            agent_id: agent_id.into(),
            agent_name: agent_name.into(),
            launch_path: launch_path.into(),
            agent_protocol: agent_protocol.into(),
            agent_launch_args,
            agent_adapter_version,
            title: title.into(),
            model,
            status: AgentConversationStatus::Idle,
            native_session_id: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            turns: Vec::new(),
        }
    }

    pub fn begin_turn(&mut self, user_message: impl Into<String>, at_ms: i64) -> &mut AgentTurn {
        let sequence = u32::try_from(self.turns.len()).unwrap_or(u32::MAX);
        self.turns
            .push(AgentTurn::new(sequence, user_message.into(), at_ms));
        self.status = AgentConversationStatus::Running;
        self.updated_at_ms = at_ms;
        self.turns.last_mut().expect("turn was just pushed")
    }

    pub fn turn_mut(&mut self, turn_id: &str) -> Option<&mut AgentTurn> {
        self.turns.iter_mut().find(|turn| turn.id == turn_id)
    }

    pub fn last_running_turn_mut(&mut self) -> Option<&mut AgentTurn> {
        self.turns
            .iter_mut()
            .rev()
            .find(|turn| turn.status == AgentTurnStatus::Running)
    }

    /// 会话标题：首轮用户输入的第一行，避免所有会话都叫“新会话”。
    pub fn ensure_title_from_first_turn(&mut self) {
        if !self.title.trim().is_empty() && self.title != "新会话" {
            return;
        }
        let Some(first) = self.turns.first() else {
            return;
        };
        let title = first
            .user_message
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        if title.is_empty() {
            return;
        }
        self.title = title.chars().take(40).collect();
    }
}
