//! Windows 下 Agent 的工作目录与权限边界声明。
//!
//! ## 权限边界（Issue #1045 第七部分）
//!
//! Windows 没有 bubblewrap 这样的无特权只读沙箱。当前实现中 Agent 以当前
//! 用户身份运行，`working_dir` 只改变当前目录，**不改变文件访问权限**。
//! Agent 若有终端/文件工具，仍能直接访问当前用户可读写的正文工作区。
//! MCP 只限制经由 MCP 协议的读写；操作系统层面不保证"AI 默认没有正文写入
//! 权限"。
//!
//! 若要提供操作系统级权限限制，需要：
//! - 给 Agent 建立真正受限的 Windows 进程身份（如 AppContainer 能力集）
//! - 或通过文件 ACL 限制 Agent 进程对正文目录的访问
//! - MCP Server 单独持有读写授权，通过消息协议代理允许的操作
//! - 仅设置 `current_dir` 或 Windows Job Object 都不能形成文件访问边界
//!
//! 该项关系到独立的进程权限设计，不在本议题中实现。当前实现保持诚实：
//! 不伪装沙箱，明确声明端侧能力边界。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::events::AgentLaunchContext;

pub struct WorkspaceAccess {
    working_dir: PathBuf,
}

/// 每个 CLI 在应用私有目录下拥有稳定、可写的工作目录。
pub fn agent_working_dir(app_data_root: &Path, agent_id: &str) -> PathBuf {
    let name: String = agent_id
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || value == '-' || value == '_' {
                value
            } else {
                '_'
            }
        })
        .collect();
    let name = if name.is_empty() {
        "default".to_string()
    } else {
        name
    };
    app_data_root.join("agent").join("workspaces").join(name)
}

impl WorkspaceAccess {
    pub fn for_context(ctx: &AgentLaunchContext) -> Result<Self, String> {
        std::fs::create_dir_all(&ctx.working_dir)
            .map_err(|e| format!("无法创建 Agent 工作目录 {}：{e}", ctx.working_dir.display()))?;
        Ok(Self {
            working_dir: ctx.working_dir.clone(),
        })
    }

    /// Windows 直接以当前用户身份启动 CLI，只固定工作目录。
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command, String> {
        if program.is_empty() {
            return Err("Agent 启动路径为空".to_string());
        }
        let mut command = Command::new(program);
        command.args(args).current_dir(&self.working_dir);
        Ok(command)
    }
}
