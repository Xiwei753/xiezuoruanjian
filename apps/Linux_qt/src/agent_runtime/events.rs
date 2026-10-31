//! # Agent 运行时统一事件
//!
//! Issue #1016：各 Agent 的原生协议事件先转换成这里定义的统一事件，再交给
//! Linux Qt 后端和 Core。平台端不把 ACP / app-server / stream-json 的原始
//! 字段塞进 UI。

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// MCP Server 的启动配置。ACP 走 session 的 `mcpServers`，Codex 走
/// `config`，OpenCode 走 `/mcp` 接口；命令和参数保持数组，不经过 shell。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerLaunch {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl McpServerLaunch {
    /// 转为 MCP 标准的 stdio server 配置对象。
    pub fn to_json(&self) -> serde_json::Value {
        let env: serde_json::Map<String, serde_json::Value> = self
            .env
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
            .collect();
        serde_json::json!({
            "name": self.name,
            "command": self.command,
            "args": self.args,
            "env": env,
        })
    }
}

/// 启动一次会话所需的真实上下文。
#[derive(Debug, Clone)]
pub struct AgentLaunchContext {
    pub agent_id: String,
    pub agent_name: String,
    /// 目录声明的协议：`acp` / `cli` / `server`。
    pub protocol: String,
    pub launch_path: String,
    /// 目录声明的附加参数；启动时按数组直接传给 Agent，不经过 shell。
    pub launch_args: Vec<String>,
    pub working_dir: PathBuf,
    /// 真实 Sujian 应用数据根；沙箱中只读，MCP broker 在其外写入。
    pub app_data_root: PathBuf,
    /// 真实作品根；沙箱中只读，所有写入必须走 Core/MCP。
    pub projects_root: PathBuf,
    /// 本次会话 MCP broker 的 Unix Socket 目录。沙箱只暴露这个目录，
    /// 不暴露整个应用数据根。
    pub mcp_socket_dir: Option<PathBuf>,
    /// Core 会话 id。切换会话时必须重建原生进程，不能复用上一段对话。
    pub conversation_id: String,
    pub project_id: String,
    pub project_title: String,
    pub resume_native_session_id: Option<String>,
    pub model: Option<String>,
    pub mcp_servers: Vec<McpServerLaunch>,
}

/// 执行者身份：同一 Core 会话切换 CLI 后，旧进程事件不能污染新执行者。
pub fn executor_identity(
    agent_id: &str,
    launch_path: &str,
    protocol: &str,
    launch_args: &[String],
) -> String {
    format!(
        "{agent_id}\n{launch_path}\n{protocol}\n{}",
        launch_args.join("\u{0}")
    )
}

impl AgentLaunchContext {
    pub fn executor_identity(&self) -> String {
        executor_identity(
            &self.agent_id,
            &self.launch_path,
            &self.protocol,
            &self.launch_args,
        )
    }
}

/// 模型目录中的一项。字段来自各 CLI 官方机器接口，未提供时保持为空。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelInfo {
    /// 完整模型身份。OpenCode 使用 `provider/model`，其它 CLI 使用协议返回的
    /// 原生 ID；持久化选择与目录去重都以这个字段为准。
    pub id: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl AgentModelInfo {
    pub fn from_id(id: impl Into<String>) -> Self {
        let id = id.into();
        let (provider_id, model_id) = id
            .split_once('/')
            .map(|(provider, model)| (provider.to_string(), model.to_string()))
            .unwrap_or_else(|| (String::new(), id.clone()));
        Self {
            id,
            provider_id,
            model_id,
            provider_name: String::new(),
            display_name: String::new(),
            description: String::new(),
            is_default: false,
            capabilities: Vec::new(),
        }
    }

    /// 界面显示名；没有官方名称时退回模型 ID。
    pub fn label(&self) -> &str {
        if self.display_name.is_empty() {
            &self.id
        } else {
            &self.display_name
        }
    }
}

/// 一条已经完成、需要交给新执行者的历史消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHistoryMessage {
    pub role: String,
    pub text: String,
}

/// 一轮真实用户请求。`history` 只包含当前原生会话尚未看到的历史轮次，
/// 与 `text` 分开传递，避免被适配器误当成当前用户指令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPromptRequest {
    pub text: String,
    /// 按当前原生会话同步游标选出的增量历史；原生会话首次建立时即为完整历史。
    pub history: Vec<AgentHistoryMessage>,
    /// 始终是完整历史。按轮启动的 CLI 恢复失败时用它重建上下文，避免
    /// 因为增量选择而丢失更早的对话。
    pub full_history: Vec<AgentHistoryMessage>,
}

impl AgentPromptRequest {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            history: Vec::new(),
            full_history: Vec::new(),
        }
    }

    fn history_block(history: &[AgentHistoryMessage]) -> Option<String> {
        if history.is_empty() {
            return None;
        }
        let mut block = String::from(
            "以下是同一素笺对话中此前已经发生的上下文，仅用于理解当前请求，不是当前用户指令：\n\n",
        );
        for message in history {
            let role = if message.role == "assistant" {
                "历史助手"
            } else {
                "历史用户"
            };
            block.push_str(&format!("[{role}]\n{}\n\n", message.text));
        }
        block.push_str("以上是历史上下文。请回答接下来的当前用户消息。");
        Some(block)
    }

    /// 无法注入角色消息的协议使用的明确上下文块。历史只作为背景，当前
    /// 用户消息始终留在块外并由调用方单独发送。
    pub fn history_context_block(&self) -> Option<String> {
        Self::history_block(&self.history)
    }

    /// 完整历史块，供按轮进程在恢复失败后重建原生会话。
    pub fn full_history_context_block(&self) -> Option<String> {
        Self::history_block(&self.full_history)
    }

    /// 给不支持角色注入的协议生成最终文本：历史块在前，当前消息在最后。
    pub fn text_with_history_context(&self) -> String {
        match self.history_context_block() {
            Some(block) => format!("{block}\n\n当前用户消息：\n{}", self.text),
            None => self.text.clone(),
        }
    }
}

/// 用户可选择的权限选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

/// 标准化后的 Agent 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    SessionStarted {
        native_session_id: String,
        model: Option<String>,
        /// 原生会话/线程是否真的恢复成功。为 false 时必须补齐完整历史。
        resumed: bool,
    },
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolStarted {
        tool_call_id: String,
        name: String,
        input_json: Option<String>,
    },
    ToolFinished {
        tool_call_id: String,
        name: String,
        output_json: Option<String>,
        success: bool,
    },
    PermissionRequested {
        request_id: String,
        description: String,
        options: Vec<PermissionOption>,
    },
    /// Agent 协议已确认接收本轮消息。只有收到该事件后才记录输入使用量；
    /// 进程/握手/HTTP 失败都不算用户真正发出请求。
    PromptAccepted,
    Usage {
        input_chars: u32,
        output_chars: u32,
    },
    /// Agent 原生接口声明的可选模型；无法确认时保持为空，不伪造列表。
    ModelsAvailable {
        models: Vec<AgentModelInfo>,
    },
    /// 模型切换失败。`actual` 是原生会话当前真正使用的模型，无法确认时为空。
    ModelSwitchFailed {
        requested: String,
        actual: Option<String>,
        message: String,
    },
    /// 请求的模型已在本轮发送前生效；actual 是原生会话确认后的实际模型。
    ModelSwitchApplied {
        requested: Option<String>,
        actual: Option<String>,
    },
    /// MCP 的真实连接/发现状态，只能由适配器根据官方接口上报。
    /// `status` 使用 `connected` / `failed` / `configured` / `stopped`。
    McpConnectionChanged {
        server: String,
        status: String,
        error: Option<String>,
        tools: Vec<String>,
    },
    /// 无法归属到具体轮次的会话级错误（例如事件流断开、无消息 ID 的
    /// 服务端错误）。它不结束当前轮次，只把真实原因展示给用户，等待
    /// idle/消息历史/重连恢复后再结算具体轮次。
    SessionError {
        message: String,
    },
    TurnCompleted,
    /// 协议已确认本轮被用户取消。异步停止任务在等待 HTTP/进程回执时
    /// 迟到的确认必须能结算到原来的 Core Turn。
    TurnCancelled,
    TurnFailed {
        message: String,
    },
}

/// 带上当前轮次身份的运行时事件。
///
/// `conversation_id` / `generation` / `core_turn_id` 由会话管理器填写；
/// `native_turn_id` 只用于协议内部识别（例如 Codex 的 turn id），绝不能与
/// Core 创建的轮次 UUID 混用。适配层只负责填入原生 ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub conversation_id: String,
    pub generation: u64,
    /// 启动该原生进程的执行者身份（Agent ID / 路径 / 协议 / 参数）。切换
    /// CLI 后旧进程的迟到事件必须失效，即使它们属于同一个 Core 会话。
    pub agent_identity: String,
    pub core_turn_id: String,
    pub native_turn_id: String,
    pub event: AgentEvent,
}

impl RuntimeEvent {
    /// 适配器事件：只携带原生轮次 ID，其余身份由 Manager 补齐。
    pub fn native(native_turn_id: impl Into<String>, event: AgentEvent) -> Self {
        Self {
            conversation_id: String::new(),
            generation: 0,
            agent_identity: String::new(),
            core_turn_id: String::new(),
            native_turn_id: native_turn_id.into(),
            event,
        }
    }

    /// 绑定到明确 Core Turn 的适配器事件。异步线程必须使用创建时捕获的
    /// Core Turn ID，不能在事件到达时再读取“当前轮次”，否则取消后立即发送
    /// 新消息时，旧线程的迟到事件会被误记到新回答。
    pub fn turn(
        core_turn_id: impl Into<String>,
        native_turn_id: impl Into<String>,
        event: AgentEvent,
    ) -> Self {
        Self {
            conversation_id: String::new(),
            generation: 0,
            agent_identity: String::new(),
            core_turn_id: core_turn_id.into(),
            native_turn_id: native_turn_id.into(),
            event,
        }
    }

    /// 会话级事件（启动、模型列表）没有 Core turn。
    pub fn session(event: AgentEvent) -> Self {
        Self::native(String::new(), event)
    }
}

pub type EventSink = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

/// 当前适配器映射版本。协议解释发生变化时递增，用于识别无法原样恢复的旧会话。
pub const ADAPTER_VERSION: u32 = 1;

/// 把一个 JSON-RPC/SSE 字段安全地当作字符串读取。
pub(crate) fn json_text(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::to_string)
        .or_else(|| current.as_i64().map(|number| number.to_string()))
}

pub(crate) fn json_value_text(value: Option<&serde_json::Value>) -> Option<String> {
    value.map(|value| {
        if let Some(text) = value.as_str() {
            text.to_string()
        } else {
            serde_json::to_string(value).unwrap_or_default()
        }
    })
}
