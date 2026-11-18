//! Windows agent_runtime 模块声明。
//!
//! 与 Linux 版本一致，额外声明 `windows_sandbox` 子模块。

pub mod acp;
pub mod adapter;
pub mod codex;
pub mod events;
pub mod mcp_binding;
pub mod model_catalog;
pub mod process_supervisor;
pub mod rpc;
pub mod session;
mod workspace_access;
pub mod windows_sandbox;

pub use adapter::CancelOutcome;
pub use events::{
    AgentEvent, AgentHistoryMessage, AgentLaunchContext, AgentModelInfo, AgentPromptRequest,
    EventSink, McpScope, McpServerLaunch, PermissionOption, RuntimeEvent, TurnStopReason,
    ADAPTER_VERSION,
};
pub use mcp_binding::{bind_current_project, McpBinding};
pub use rpc::CloseReason;
pub use session::AgentSessionManager;
pub use workspace_access::agent_working_dir;
