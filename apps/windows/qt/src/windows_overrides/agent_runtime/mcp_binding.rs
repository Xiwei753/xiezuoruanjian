//! 当前作品与 MCP Server 的绑定（Windows）。
//!
//! Windows 不使用 Unix socket broker：Agent 以当前用户身份运行，直接启动
//! `sujian_mcp.exe` 的 stdio 入口。写事务仍由 Core 的权限接口执行。

use std::path::{Path, PathBuf};

use super::events::{AgentLaunchContext, McpScope, McpServerLaunch};

#[derive(Debug, Clone)]
pub struct McpBinding {
    pub launch: Option<McpServerLaunch>,
    pub command: Option<String>,
    pub status: String,
}

fn find_in_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for name in [
            "sujian_mcp.exe",
            "sujian-mcp.exe",
            "sujian_mcp",
            "sujian-mcp",
        ] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_command() -> Option<String> {
    if let Some(override_path) = std::env::var_os("SUJIAN_MCP_BIN") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            for name in ["sujian_mcp.exe", "sujian-mcp.exe"] {
                let candidate = parent.join(name);
                if candidate.is_file() {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }
    find_in_path().map(|path| path.to_string_lossy().into_owned())
}

/// 按当前作品与星图作用域构造 MCP stdio 启动配置。
pub fn bind_current_project(
    app_data_root: &str,
    projects_root: &str,
    scope: &McpScope,
) -> McpBinding {
    let Some(command) = resolve_command() else {
        return McpBinding {
            launch: None,
            command: None,
            status: "未找到随包安装的 sujian_mcp.exe".to_string(),
        };
    };
    let path_segments = if scope.path_segments_json.trim().is_empty() {
        "[]".to_string()
    } else {
        scope.path_segments_json.clone()
    };

    let mut args = vec![
        "--app-data-root".to_string(),
        app_data_root.to_string(),
        "--projects-root".to_string(),
        projects_root.to_string(),
        "--starmap-path".to_string(),
        path_segments.clone(),
    ];
    if !scope.project_id.is_empty() {
        args.push("--project-id".to_string());
        args.push(scope.project_id.clone());
    }
    if !scope.root_starmap_id.is_empty() {
        args.push("--root-starmap-id".to_string());
        args.push(scope.root_starmap_id.clone());
    }
    if !scope.active_starmap_id.is_empty() {
        args.push("--active-starmap-id".to_string());
        args.push(scope.active_starmap_id.clone());
    }
    if !scope.route_key.is_empty() {
        args.push("--starmap-route".to_string());
        args.push(scope.route_key.clone());
    }
    if !scope.executor.is_empty() {
        args.push("--agent-identity".to_string());
        args.push(scope.executor.clone());
    }

    let launch = McpServerLaunch {
        name: "sujian".to_string(),
        command: command.clone(),
        args,
        env: vec![
            (
                "SUJIAN_APP_DATA_ROOT".to_string(),
                app_data_root.to_string(),
            ),
            (
                "SUJIAN_PROJECTS_ROOT".to_string(),
                projects_root.to_string(),
            ),
            ("SUJIAN_PROJECT_ID".to_string(), scope.project_id.clone()),
            (
                "SUJIAN_ROOT_STARMAP_ID".to_string(),
                scope.root_starmap_id.clone(),
            ),
            (
                "SUJIAN_ACTIVE_STARMAP_ID".to_string(),
                scope.active_starmap_id.clone(),
            ),
            ("SUJIAN_STARMAP_PATH".to_string(), path_segments),
            ("SUJIAN_STARMAP_ROUTE".to_string(), scope.route_key.clone()),
            ("SUJIAN_AGENT_ID".to_string(), scope.executor.clone()),
        ],
    };
    let status = if scope.root_starmap_id.is_empty() {
        "sujian-mcp 已绑定当前作品".to_string()
    } else {
        let active = if scope.active_starmap_id.is_empty() {
            &scope.root_starmap_id
        } else {
            &scope.active_starmap_id
        };
        format!("sujian-mcp 已绑定当前作品与星图 {active}")
    };
    McpBinding {
        launch: Some(launch),
        command: Some(command),
        status,
    }
}

/// Windows 不需要 broker；保留类型让共享 `session.rs` 无需分叉。
pub struct McpBroker {
    directory: PathBuf,
}

impl McpBroker {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

pub fn prepare_mcp_brokers(
    ctx: &AgentLaunchContext,
) -> Result<(Vec<McpServerLaunch>, Option<McpBroker>), String> {
    Ok((ctx.mcp_servers.clone(), None))
}
