//! Windows 下 Agent 的工作目录。
//!
//! Windows 没有 bubblewrap 这样的无特权只读沙箱。这里不伪装沙箱：Agent 以
//! 当前用户身份运行，工作目录固定在应用私有目录，正文读写仍通过
//! `sujian-mcp` 的权限接口完成。比赛博朋克式的“假 bwrap”更诚实。

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
