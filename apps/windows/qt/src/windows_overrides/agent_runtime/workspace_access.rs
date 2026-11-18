//! Windows 下 Agent 的工作目录与权限边界。
//!
//! Issue #1045 第七部分：Windows AppContainer 沙箱实现。
//!
//! `WorkspaceAccess::command()` 不再直接启动 CLI，而是启动
//! `sujian-agent-host.exe`——一个可信原生启动器，它：
//! 1. 创建 AppContainer profile（每个 Agent ID 一个持久 profile）
//! 2. 为工作目录、CLI 安装目录、CLI 配置目录设置 ACL
//! 3. 在 AppContainer 中启动 CLI，只继承 stdio 管道句柄
//! 4. 转发 stdin/stdout/stderr，管理进程生命周期
//!
//! 正文工作区、应用数据根不在 ACL 中，Agent 只能通过 MCP broker 代理读写。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::events::AgentLaunchContext;

pub struct WorkspaceAccess {
    working_dir: PathBuf,
    agent_id: String,
    launch_path: String,
    readable_dirs: Vec<PathBuf>,
    writable_dirs: Vec<PathBuf>,
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

fn resolve_agent_host() -> Result<String, String> {
    if let Some(override_path) = std::env::var_os("SUJIAN_AGENT_HOST_BIN") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Ok(path.to_string_lossy().into_owned());
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let candidate = parent.join("sujian-agent-host.exe");
            if candidate.is_file() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }
    Err("未找到 sujian-agent-host.exe".to_string())
}

fn agent_config_dirs(agent_id: &str, home: &Path) -> Vec<PathBuf> {
    let names: Vec<&str> = match agent_id {
        "codex" => vec![".codex"],
        "opencode" => vec![".opencode", ".config/opencode", ".local/share/opencode"],
        _ => Vec::new(),
    };

    let mut dirs = Vec::new();
    for name in names {
        dirs.push(home.join(name));
    }
    dirs
}

impl WorkspaceAccess {
    pub fn for_context(ctx: &AgentLaunchContext) -> Result<Self, String> {
        std::fs::create_dir_all(&ctx.working_dir)
            .map_err(|e| format!("无法创建 Agent 工作目录 {}：{e}", ctx.working_dir.display()))?;

        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| PathBuf::from("C:\\Users\\Default"));

        let mut readable_dirs = Vec::new();
        let mut writable_dirs = Vec::new();

        writable_dirs.push(ctx.working_dir.clone());

        for dir in agent_config_dirs(&ctx.agent_id, &home) {
            if dir.exists() || !dir.as_os_str().is_empty() {
                writable_dirs.push(dir);
            }
        }

        if let Some(parent) = Path::new(&ctx.launch_path).parent() {
            readable_dirs.push(parent.to_path_buf());
        }

        for server in &ctx.mcp_servers {
            let path = PathBuf::from(&server.command);
            if let Some(parent) = path.parent() {
                readable_dirs.push(parent.to_path_buf());
            }
        }

        Ok(Self {
            working_dir: ctx.working_dir.clone(),
            agent_id: ctx.agent_id.clone(),
            launch_path: ctx.launch_path.clone(),
            readable_dirs,
            writable_dirs,
        })
    }

    /// 启动 `sujian-agent-host.exe`，由它在 AppContainer 中启动 CLI。
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command, String> {
        if program.is_empty() {
            return Err("Agent 启动路径为空".to_string());
        }

        let agent_host = resolve_agent_host()?;

        let mut command = Command::new(&agent_host);
        command
            .arg("--agent-id")
            .arg(&self.agent_id)
            .arg("--working-dir")
            .arg(&self.working_dir)
            .arg("--launch-path")
            .arg(program);

        for dir in &self.readable_dirs {
            command.arg("--readable-dir").arg(dir);
        }
        for dir in &self.writable_dirs {
            command.arg("--writable-dir").arg(dir);
        }

        command.arg("--");
        command.args(args);

        Ok(command)
    }
}
