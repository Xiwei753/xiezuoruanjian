//! # Agent 会话
//!
//! Issue #1016：Core 保存 AI 会话事实（用户输入、模型输出、工具事件、
//! 原生会话身份）。进程启动、协议流式事件和权限交互属于 Linux Qt 平台端。

pub mod conversation;
pub mod conversation_store;

pub use conversation::{
    AgentConversation, AgentConversationStatus, AgentMessagePart, AgentToolEvent, AgentTurn,
    AgentTurnStatus,
};
pub use conversation_store::AgentConversationStore;
