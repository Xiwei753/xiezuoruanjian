//! Linux Qt 的 Agent 运行时。
//!
//! Issue #1022：只管理 OpenCode（官方 ACP）与 Codex（官方 app-server）的
//! 真实机器会话，把原生事件标准化后交给 QML 与 Core。发现和选择仍由
//! `agent_cli` 负责。
//!
//! 模块边界：
//! - `events`：统一事件与启动上下文；
//! - `adapter` / `session`：适配器契约与单会话生命周期；
//! - `acp` / `codex`：两个受支持 CLI 的官方接口；
//! - `mcp_binding`：当前作品的 `sujian-mcp` 启动配置；
//! - `rpc`：ACP 与 Codex 共用的 stdio JSON-RPC 客户端。

pub mod acp;
pub mod adapter;
pub mod codex;
pub mod events;
pub mod mcp_binding;
pub mod model_catalog;
pub mod rpc;
pub mod session;
mod workspace_access;

pub use adapter::CancelOutcome;
pub use events::{
    AgentEvent, AgentHistoryMessage, AgentLaunchContext, AgentModelInfo, AgentPromptRequest,
    EventSink, McpServerLaunch, PermissionOption, RuntimeEvent, ADAPTER_VERSION,
};
pub use mcp_binding::{bind_current_project, McpBinding};
pub use rpc::CloseReason;
pub use session::AgentSessionManager;
pub use workspace_access::agent_working_dir;
