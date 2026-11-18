//! 当前作品与 MCP Server 的绑定（Windows）。
//!
//! Issue #1045：Windows AppContainer 沙箱实现。
//!
//! MCP broker 在沙箱外启动（1`sujian_mcp.exe` 以 stdio 方式运行），
//! 通过 named pipe 与沙箱内的 `sujian-mcp-bridge.exe` 通信。
//! 沙箱内 CLI 看到的 MCP 配置指向 bridge，不含任何可绕过 Core 的写权限。

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, ConnectNamedPipe, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE,
    PIPE_READMODE_BYTE, PIPE_WAIT,
};

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
        for name in ["sujian_mcp.exe", "sujian-mcp.exe", "sujian_mcp", "sujian-mcp"] {
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

fn resolve_bridge_command() -> Option<String> {
    if let Some(override_path) = std::env::var_os("SUJIAN+MCP_BRIDGE_BIN") {
        let path = PathBuf::from(override_path);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let candidate = parent.join("sujian-mcp-bridge.exe");
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
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
            ("SUJIAN_APP_DATA_ROOT".to_string(), app_data_root.to_string()),
            ("SUJIAN_PROJECTS_ROOT".to_string(), projects_root.to_string()),
            ("SUJIAN_PROJECT_ID".to_string(), scope.project_id.clone()),
            ("SUJIAN_ROOT_STARMAP_ID".to_string(), scope.root_starmap_id.clone()),
            ("SUJIAN_ACTIVE_STARMAP_ID".to_string(), scope.active_starmap_id.clone()),
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

/// 沙箱外的 MCP broker 生命周期。
///
/// Agent 在 AppContainer 沙箱里运行，它启动的 `sujian-mcp-bridge.exe` 连接到
/// 这里创建的 named pipe；真正的 `sujian_mcp.exe` broker 在沙箱外以 stdio
/// 方式运行，通过 named pipe 与 bridge 双向转发 JSON-RPC。
pub struct McpBroker {
    children: Vec<Child>,
}

impl McpBroker {
    pub fn directory(&self) -> &Path {
        Path::new("")
    }
}

impl Drop for McpBroker {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// 把当前会话的 MCP 配置拆成"沙箱外 broker + 沙箱内 named pipe bridge"。
/// 返回给适配器的服务器配置只包含 bridge 命令，不含任何可绕过 Core 的写权限。
pub fn prepare_mcp_brokers(
    ctx: &AgentLaunchContext,
) -> Result<(Vec<McpServerLaunch>, Option<McpBroker>), String> {
    if ctx.mcp_servers.is_empty() {
        return Ok((Vec::new(), None));
    }

    let bridge_command = resolve_bridge_command().ok_or_else(|| {
        "未找到 sujian-mcp-bridge.exe，无法建立 MCP named pipe 桥".to_string()
    })?;

    let mut children = Vec::new();
    let mut bridges = Vec::new();

    for (index, server) in ctx.mcp_servers.iter().enumerate() {
        let pipe_name = format!("Sujian.Mcp.{}.{}", uuid::Uuid::new_v4(), index);
        let pipe_path = format!(r"\\.\pipe\{}", pipe_name);
        let pipe_wide = wide(&pipe_path);

        let pipe_handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                65536,
                65536,
                0,
                ptr::null(),
            )
        };

        if pipe_handle == INVALID_HANDLE_VALUE {
            for c in &mut children {
                let _ = c.kill();
                let _ = c.wait();
            }
            return Err(format!("CreateNamedPipeW 失败: {}", pipe_path));
        }

        let mut broker_cmd = Command::new(&server.command);
        broker_cmd
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .envs(server.env.iter().cloned());

        let mut child = broker_cmd
            .spawn()
            .map_err(|e| format!("无法启动作品 MCP broker（{}）：{e}", server.command))?;

        let broker_stdin = child.stdin.take();
        let broker_stdout = child.stdout.take();

        let pipe_path_for_relay = pipe_path.clone();
        let pipe_handle_for_connect = pipe_handle;
        let pipe_handle_for_relay = pipe_handle;

        thread::spawn(move || {
            unsafe {
                let _ = ConnectNamedPipe(pipe_handle_for_connect, ptr::null_mut());
            }

            let pipe_file = unsafe { std::fs::File::from_raw_handle(pipe_handle_for_relay.0 as *mut _) };

            let pipe_for_read = pipe_file.try_clone().unwrap_or_else(|_| {
                unsafe { std::fs::File::from_raw_handle(pipe_handle_for_relay.0 as *mut _) }
            });
            let pipe_for_write = pipe_file;

            let stdin_handle = thread::spawn(move || {
                if let Some(mut broker_stdin) = broker_stdin {
                    let mut pipe = pipe_for_read;
                    let mut buf = [0u8; 4096];
                    loop {
                        match pipe.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                if broker_stdin.write_all(&buf[..n]).is_err() {
                                    break;
                                }
                                let _ = broker_stdin.flush();
                            }
                            Err(_) => break,
                        }
                    }
                }
            });

            let stdout_handle = thread::spawn(move || {
                if let Some(mut broker_stdout) = broker_stdout {
                    let mut pipe = pipe_for_write;
                    let mut buf = [0u8; 4096];
                    loop {
                        match broker_stdout.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                if pipe.write_all(&buf[..n]).is_err() {
                                    break;
                                }
                                let _ = pipe.flush();
                            }
                            Err(_) => break,
                        }
                    }
                }
            });

            stdin_handle.join().ok();
            stdout_handle.join().ok();

            unsafe { let _ = CloseHandle(pipe_handle_for_relay); }
            let _ = pipe_path_for_relay;
        });

        children.push(child);

        bridges.push(McpServerLaunch {
            name: server.name.clone(),
            command: bridge_command.clone(),
            args: vec!["--pipe".to_string(), pipe_path.clone()],
            env: server.env.clone(),
        });
    }

    Ok((
        bridges,
        Some(McpBroker { children }),
    ))
}
