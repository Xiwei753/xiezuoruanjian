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
    /// 本轮实际执行的 CLI。旧会话文件没有这些字段时为空，由会话级
    /// Agent 身份兜底。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub launch_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_protocol: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_launch_args: Vec<String>,
    /// 本轮实际请求的模型。CLI 未显式选择时为空，表示使用默认模型。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 本轮对应的原生 session / thread。跨 CLI 切换后只恢复同一执行者的
    /// 原生会话，绝不把别的 CLI 的 ID 传给当前进程。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
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
            agent_id: String::new(),
            agent_name: String::new(),
            launch_path: String::new(),
            agent_protocol: String::new(),
            agent_launch_args: Vec::new(),
            model: None,
            native_session_id: None,
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

/// 某个 CLI 安装在此会话中的原生 session 映射。
///
/// 同一个素笺对话可以连续切换不同 CLI；每个 CLI 的原生会话身份互相独立，
/// 恢复时只能使用当前执行者自己的映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNativeSession {
    pub agent_id: String,
    #[serde(default)]
    pub launch_path: String,
    pub native_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 已经确定进入该原生会话的最后一轮素笺对话。下一轮从这里之后补充
    /// 上下文；原生会话重建时清空，重新同步完整历史。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synchronized_turn_id: Option<String>,
    pub updated_at_ms: i64,
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
    /// 最近一次实际使用的 Agent 原生会话 / 线程 ID。保留该字段兼容旧
    /// 会话文件；新逻辑优先使用 [`Self::native_sessions`] 中按执行者保存
    /// 的映射。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    /// 按 Agent 安装分别保存的原生会话。缺失时回退到上面的旧字段，但只在
    /// 当前执行者与会话最后使用者的身份一致时才允许恢复。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_sessions: Vec<AgentNativeSession>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub turns: Vec<AgentTurn>,
}

/// 当前会话文件格式。旧文件缺失版本时按 1 读取；新增字段均带 serde 默认值。
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
            schema_version: CURRENT_SCHEMA_VERSION,
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
            native_sessions: Vec::new(),
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

    /// 当前执行者在此会话中保存的原生 session。只按 Agent ID + 真实启动
    /// 路径匹配，防止同名 CLI 的不同安装混用线程。
    pub fn native_session_for(&self, agent_id: &str, launch_path: &str) -> Option<&str> {
        self.native_session(agent_id, launch_path)
            .map(|session| session.native_session_id.as_str())
    }

    pub fn native_session(&self, agent_id: &str, launch_path: &str) -> Option<&AgentNativeSession> {
        self.native_sessions
            .iter()
            .rev()
            .find(|session| session.agent_id == agent_id && session.launch_path == launch_path)
    }

    /// 记录当前执行者最新的原生 session。保留有限条数，避免长期切换 CLI
    /// 让会话文件无限增长。
    pub fn record_native_session(
        &mut self,
        agent_id: &str,
        launch_path: &str,
        native_session_id: &str,
        model: Option<String>,
        last_synchronized_turn_id: Option<String>,
        at_ms: i64,
    ) {
        self.native_sessions
            .retain(|session| session.agent_id != agent_id || session.launch_path != launch_path);
        self.native_sessions.push(AgentNativeSession {
            agent_id: agent_id.to_string(),
            launch_path: launch_path.to_string(),
            native_session_id: native_session_id.to_string(),
            model,
            last_synchronized_turn_id,
            updated_at_ms: at_ms,
        });
        const MAX_NATIVE_SESSIONS: usize = 16;
        if self.native_sessions.len() > MAX_NATIVE_SESSIONS {
            let excess = self.native_sessions.len() - MAX_NATIVE_SESSIONS;
            self.native_sessions.drain(0..excess);
        }
    }

    pub fn set_executor_model(
        &mut self,
        agent_id: &str,
        launch_path: &str,
        model: Option<String>,
        at_ms: i64,
    ) {
        if let Some(session) = self
            .native_sessions
            .iter_mut()
            .rev()
            .find(|session| session.agent_id == agent_id && session.launch_path == launch_path)
        {
            session.model = model;
            session.updated_at_ms = at_ms;
        }
    }

    pub fn set_native_session_synchronized_turn(
        &mut self,
        agent_id: &str,
        launch_path: &str,
        turn_id: Option<String>,
        at_ms: i64,
    ) {
        if let Some(session) = self
            .native_sessions
            .iter_mut()
            .rev()
            .find(|session| session.agent_id == agent_id && session.launch_path == launch_path)
        {
            session.last_synchronized_turn_id = turn_id;
            session.updated_at_ms = at_ms;
        }
    }

    /// 旧版会话只有会话级 Agent 身份。首次加载时把该身份固定到每一轮，
    /// 并迁移旧的原生 session，避免后续切换 CLI 时追溯改写历史轮次。
    pub fn migrate_legacy(&mut self) -> bool {
        let mut changed = false;
        if self.schema_version < CURRENT_SCHEMA_VERSION {
            self.schema_version = CURRENT_SCHEMA_VERSION;
            changed = true;
        }
        let legacy_agent_id = self.agent_id.clone();
        let legacy_agent_name = self.agent_name.clone();
        let legacy_launch_path = self.launch_path.clone();
        let legacy_protocol = self.agent_protocol.clone();
        let legacy_args = self.agent_launch_args.clone();
        let legacy_model = self.model.clone();
        for turn in &mut self.turns {
            if !turn.agent_id.is_empty() {
                continue;
            }
            turn.agent_id = legacy_agent_id.clone();
            turn.agent_name = legacy_agent_name.clone();
            turn.launch_path = legacy_launch_path.clone();
            turn.agent_protocol = legacy_protocol.clone();
            turn.agent_launch_args = legacy_args.clone();
            if turn.model.is_none() {
                turn.model = legacy_model.clone();
            }
            changed = true;
        }
        if self.native_sessions.is_empty() {
            if let Some(native_session_id) = self.native_session_id.clone() {
                if !legacy_agent_id.is_empty() {
                    let synchronized = self
                        .turns
                        .iter()
                        .rev()
                        .find(|turn| {
                            turn.agent_id == legacy_agent_id
                                && turn.launch_path == legacy_launch_path
                                && turn.status == AgentTurnStatus::Completed
                        })
                        .map(|turn| turn.id.clone());
                    self.record_native_session(
                        &legacy_agent_id,
                        &legacy_launch_path,
                        &native_session_id,
                        legacy_model,
                        synchronized,
                        self.updated_at_ms,
                    );
                    changed = true;
                }
            }
        }
        changed
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
