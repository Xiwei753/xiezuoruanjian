//! Linux Qt 的 Agent 运行时。
//!
//! Issue #1016：统一管理已安装 Agent 的真实机器会话（ACP / Codex app-server
//! / OpenCode Server / Claude、Gemini stream-json），把原生事件标准化后交给
//! QML 与 Core。发现和选择仍由 `agent_cli` 负责。
//!
//! 模块边界：
//! - `events`：统一事件与启动上下文；
//! - `adapter` / `session`：适配器契约与单会话生命周期；
//! - `acp` / `codex` / `opencode` / `claude` / `gemini`：各 Agent 官方接口；
//! - `mcp_binding`：当前作品的 `sujian-mcp` 启动配置；
//! - `rpc`：ACP 与 Codex 共用的 stdio JSON-RPC 客户端。

pub mod acp;
pub mod adapter;
pub mod claude;
pub mod codex;
pub mod events;
pub mod gemini;
pub mod mcp_binding;
pub mod opencode;
pub mod rpc;
pub mod session;
mod workspace_access;

pub use events::{
    AgentEvent, AgentLaunchContext, EventSink, McpServerLaunch, PermissionOption, RuntimeEvent,
    ADAPTER_VERSION,
};
pub use mcp_binding::{bind_current_project, McpBinding};
pub use session::AgentSessionManager;
