//! 当前作品与 MCP Server 的绑定。
//!
//! Issue #1016：把真实 WriterCore 工作区路径注入 `sujian_mcp`，再由各 Agent
//! 的正式 MCP 配置机制随会话传入。原生包使用稳定安装入口；AppImage 使用
//! 当前 AppImage 绝对路径 + `--sujian-mcp` 转发（由 AppRun 处理），不依赖
//! 临时挂载路径。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::events::{AgentLaunchContext, McpServerLaunch};

#[derive(Debug, Clone)]
pub struct McpBinding {
    pub launch: Option<McpServerLaunch>,
    pub command: Option<String>,
    pub status: String,
}

fn executable(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
}

fn find_in_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for name in ["sujian-mcp", "sujian_mcp"] {
            let candidate = directory.join(name);
            if executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// 找到本次安装可用的稳定 MCP 入口。
fn resolve_command() -> Option<(String, Vec<String>)> {
    // AppImage：不能使用 /tmp 挂载目录。APPIMAGE 是用户实际打开的绝对路径。
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(appimage);
        if executable(&path) {
            return Some((
                path.to_string_lossy().into_owned(),
                vec!["--sujian-mcp".to_string()],
            ));
        }
    }
    if let Some(override_path) = std::env::var_os("SUJIAN_MCP_BIN") {
        let path = PathBuf::from(override_path);
        if executable(&path) {
            return Some((path.to_string_lossy().into_owned(), Vec::new()));
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            for name in ["sujian-mcp", "sujian_mcp"] {
                let candidate = parent.join(name);
                if executable(&candidate) {
                    return Some((candidate.to_string_lossy().into_owned(), Vec::new()));
                }
            }
        }
    }
    for candidate in [
        "/usr/libexec/sujian/sujian_mcp",
        "/usr/bin/sujian-mcp",
        "/usr/local/bin/sujian-mcp",
    ] {
        let path = PathBuf::from(candidate);
        if executable(&path) {
            return Some((candidate.to_string(), Vec::new()));
        }
    }
    find_in_path().map(|path| (path.to_string_lossy().into_owned(), Vec::new()))
}

/// 按当前作品构造 MCP stdio 启动配置。
pub fn bind_current_project(
    app_data_root: &str,
    projects_root: &str,
    project_id: &str,
) -> McpBinding {
    let Some((command, mut args)) = resolve_command() else {
        return McpBinding {
            launch: None,
            command: None,
            status: "未找到随包安装的 sujian-mcp".to_string(),
        };
    };
    args.push("--app-data-root".to_string());
    args.push(app_data_root.to_string());
    args.push("--projects-root".to_string());
    args.push(projects_root.to_string());
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
            ("SUJIAN_PROJECT_ID".to_string(), project_id.to_string()),
        ],
    };
    McpBinding {
        launch: Some(launch),
        command: Some(command),
        status: "sujian-mcp 已绑定当前作品".to_string(),
    }
}

/// 沙箱外的 MCP broker 生命周期。
///
/// Agent 在只读作品沙箱里运行，它启动的 `sujian-mcp` 只能作为 stdio bridge
/// 连接到这里先行启动的 broker；真正的 Core 写事务发生在沙箱之外。
pub struct McpBroker {
    children: Vec<Child>,
    directory: PathBuf,
}

impl Drop for McpBroker {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn socket_ready(path: &Path) -> bool {
    path.exists()
        && std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// 把当前会话的 MCP 配置拆成“沙箱外 broker + 沙箱内 stdio bridge”。
/// 返回给适配器的服务器配置只包含 bridge 命令，不含任何可绕过 Core 的写权限。
pub fn prepare_mcp_brokers(
    ctx: &AgentLaunchContext,
) -> Result<(Vec<McpServerLaunch>, Option<McpBroker>), String> {
    if ctx.mcp_servers.is_empty() {
        return Ok((Vec::new(), None));
    }
    let directory = ctx
        .app_data_root
        .join("agent")
        .join("mcp")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("创建 MCP broker 目录失败：{error}"))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("设置 MCP broker 目录权限失败：{error}"))?;
    }

    let mut children = Vec::new();
    let mut bridges = Vec::new();
    for (index, server) in ctx.mcp_servers.iter().enumerate() {
        let socket = directory.join(format!("{index}-{}.sock", server.name));
        let mut command = Command::new(&server.command);
        command
            .args(&server.args)
            .arg("--unix-socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .envs(server.env.iter().cloned());
        let child = command.spawn().map_err(|error| {
            format!(
                "无法启动作品 MCP broker（{}）：{error}",
                server.command
            )
        })?;
        children.push(child);
        let started = Instant::now();
        while !socket_ready(&socket) {
            if started.elapsed() > Duration::from_secs(5) {
                let mut broker = McpBroker {
                    children,
                    directory,
                };
                broker.children.iter_mut().for_each(|child| {
                    let _ = child.kill();
                });
                return Err("作品 MCP broker 启动超时".to_string());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut bridge_args = server.args.clone();
        bridge_args.push("--stdio-bridge".to_string());
        bridge_args.push(socket.to_string_lossy().into_owned());
        bridges.push(McpServerLaunch {
            name: server.name.clone(),
            command: server.command.clone(),
            args: bridge_args,
            env: server.env.clone(),
        });
    }
    Ok((
        bridges,
        Some(McpBroker {
            children,
            directory,
        }),
    ))
}
