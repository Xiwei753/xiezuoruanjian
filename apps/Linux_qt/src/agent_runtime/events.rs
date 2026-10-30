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
    Usage {
        input_chars: u32,
        output_chars: u32,
    },
    /// Agent 原生接口声明的可选模型；无法确认时保持为空，不伪造列表。
    ModelsAvailable {
        models: Vec<String>,
    },
    TurnCompleted,
    TurnFailed {
        message: String,
    },
}

/// 带上当前轮次身份的运行时事件。轮次由 Core 创建，适配层不自行生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub turn_id: String,
    pub event: AgentEvent,
}

pub type EventSink = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

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
