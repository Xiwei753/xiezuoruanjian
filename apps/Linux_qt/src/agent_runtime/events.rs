//! # Agent 运行时统一事件
//!
//! Issue #1016：各 Agent 的原生协议事件先转换成这里定义的统一事件，再交给
//! Linux Qt 后端和 Core。平台端不把 ACP / app-server / stream-json 的原始
//! 字段塞进 UI。

use std::path::PathBuf;
use std::sync::Arc;

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
        models: Vec<String>,
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
