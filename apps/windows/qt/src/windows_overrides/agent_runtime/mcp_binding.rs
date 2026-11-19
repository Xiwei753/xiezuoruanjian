//! 当前作品与 MCP Server 的绑定（Windows）。
//!
//! Issue #1045：Windows AppContainer 沙箱实现。
//!
//! MCP broker 在沙箱外启动（`sujian_mcp.exe` 以 stdio 方式运行），
//! 通过 named pipe 与沙箱内的 `sujian-mcp-bridge.exe` 通信。
//! 沙箱内 CLI 看到的 MCP 配置指向 bridge，不含任何可绕过 Core 的写权限。
//!
//! Named pipe 使用 `\\.\pipe\LOCAL\...` 命名空间（AppContainer 要求），
//! 并通过 ACL 显式授予对应 AppContainer SID 连接权限。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, ERROR_PIPE_CONNECTED,
};
use windows::Win32::Security::{
    InitializeSecurityDescriptor, PACL, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    SetSecurityDescriptorDacl, SECURITY_DESCRIPTOR,
};
use windows::Win32::Security::Authorization::{
    SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_GROUP,
    TRUSTEE_W,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE,
    PIPE_READMODE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    CancelSynchronousIo, GetCurrentProcess, GetCurrentThread,
};

use super::events::{AgentLaunchContext, McpScope, McpServerLaunch};
use super::windows_sandbox::AppContainerProfile;

const FILE_GENERIC_READ: u32 = 0x120089;
const FILE_GENERIC_WRITE: u32 = 0x120116;

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
    if let Some(override_path) = std::env::var_os("SUJIAN_MCP_BRIDGE_BIN") {
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

/// 持有 named pipe 安全描述符和 ACL 的 RAII 结构。
///
/// `SECURITY_DESCRIPTOR` 和 ACL 在 `CreateNamedPipeW` 期间必须保持存活，
/// `Drop` 时释放 `LocalFree` 分配的 ACL。
struct PipeSecurity {
    sd: Box<SECURITY_DESCRIPTOR>,
    acl: PACL,
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self.acl.0.is_null() {
            unsafe { let _ = windows::Win32::Foundation::LocalFree(self.acl.0 as *mut _); }
        }
    }
}

/// 为 AppContainer SID 创建命名管道的安全描述符。
///
/// 授予当前 AppContainer SID `FILE_GENERIC_READ | FILE_GENERIC_WRITE` 权限，
/// 确保 bridge 能双向读写管道。
fn create_pipe_security(
    sid_ptr: *mut core::ffi::c_void,
) -> Result<(SECURITY_ATTRIBUTES, PipeSecurity), String> {
    let explicit_access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        grfAccessMode: SET_ACCESS,
        grfInheritance: 0,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            ptstrName: sid_ptr,
            ..Default::default()
        },
    };

    let mut new_acl: PACL = ptr::null_mut();
    unsafe {
        let result = SetEntriesInAclW(1, &explicit_access, ptr::null(), &mut new_acl);
        if result != 0 {
            return Err(format!("SetEntriesInAclW: error {result}"));
        }
    }

    let mut sd = Box::new(SECURITY_DESCRIPTOR::default());
    unsafe {
        InitializeSecurityDescriptor(
            PSECURITY_DESCRIPTOR(sd.as_mut() as *mut _ as *mut _),
            1,
        )
        .map_err(|e| format!("InitializeSecurityDescriptor: {e}"))?;

        SetSecurityDescriptorDacl(
            PSECURITY_DESCRIPTOR(sd.as_mut() as *mut _ as *mut _),
            true,
            Some(new_acl),
            false,
        )
        .map_err(|e| format!("SetSecurityDescriptorDacl: {e}"))?;
    }

    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_mut() as *mut _ as *mut _,
        bInheritHandle: windows::Win32::Foundation::BOOL(0),
    };

    Ok((sa, PipeSecurity { sd, acl: new_acl }))
}

/// 单个 MCP relay 的所有资源，统一生命周期管理。
///
/// 持有原始 pipe handle、relay 线程使用的 duplicate handle、
/// relay 线程的 Win32 线程句柄（用于 `CancelSynchronousIo`）、
/// Rust `JoinHandle`、broker 子进程和 pipe 安全描述符。
struct RelayWorker {
    pipe_handle: HANDLE,
    relay_handle: HANDLE,
    thread_handle: HANDLE,
    relay_thread: Option<JoinHandle<()>>,
    child: Child,
    _pipe_security: PipeSecurity,
}

impl RelayWorker {
    /// 取消并回收 relay 线程：先 kill 子进程让已连接的 I/O 失败，
    /// 再 `CancelSynchronousIo` 取消阻塞的 `ConnectNamedPipe`，
    /// 最后 join relay 线程。
    fn cancel_and_join(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        unsafe { let _ = CancelSynchronousIo(self.thread_handle); }
        if let Some(thread) = self.relay_thread.take() {
            let _ = thread.join();
        }
    }

    fn close_handles(&self) {
        unsafe {
            let _ = CloseHandle(self.thread_handle);
            let _ = CloseHandle(self.pipe_handle);
        }
    }
}

/// 沙箱外的 MCP broker 生命周期。
///
/// Agent 在 AppContainer 沙箱里运行，它启动的 `sujian-mcp-bridge.exe` 连接到
/// 这里创建的 named pipe；真正的 `sujian_mcp.exe` broker 在沙箱外以 stdio
/// 方式运行，通过 named pipe 与 bridge 双向转发 JSON-RPC。
pub struct McpBroker {
    workers: Vec<RelayWorker>,
}

impl McpBroker {
    pub fn directory(&self) -> &Path {
        Path::new("")
    }
}

impl Drop for McpBroker {
    fn drop(&mut self) {
        for worker in &mut self.workers {
            worker.cancel_and_join();
        }
        for worker in &self.workers {
            worker.close_handles();
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

    let profile = AppContainerProfile::for_agent(&ctx.agent_id)
        .map_err(|e| format!("创建 AppContainer profile 失败: {e}"))?;

    let mut bridges = Vec::new();
    let mut workers = Vec::new();

    for (index, server) in ctx.mcp_servers.iter().enumerate() {
        let pipe_name = format!(
            "LOCAL\\Sujian.Mcp.{}.{}",
            uuid::Uuid::new_v4(),
            index
        );
        let pipe_path = format!(r"\\.\pipe\{}", pipe_name);
        let pipe_wide = wide(&pipe_path);

        let (sa, security) = create_pipe_security(profile.sid().as_ptr())?;

        let pipe_handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                65536,
                65536,
                0,
                Some(&sa),
            )
        };

        if pipe_handle == INVALID_HANDLE_VALUE {
            drop(security);
            for w in &mut workers {
                w.cancel_and_join();
                w.close_handles();
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

        let child = broker_cmd
            .spawn()
            .map_err(|e| {
                drop(security);
                unsafe { let _ = CloseHandle(pipe_handle); }
                for w in &mut workers {
                    w.cancel_and_join();
                    w.close_handles();
                }
                format!("无法启动作品 MCP broker（{}）：{e}", server.command)
            })?;

        let broker_stdin = child.stdin.take();
        let broker_stdout = child.stdout.take();

        let relay_handle = duplicate_handle(pipe_handle).map_err(|e| {
            drop(security);
            let _ = child.kill();
            let _ = child.wait();
            unsafe { let _ = CloseHandle(pipe_handle); }
            for w in &mut workers {
                w.cancel_and_join();
                w.close_handles();
            }
            e
        })?;

        let (thread_handle_tx, thread_handle_rx) = mpsc::channel::<HANDLE>();

        let relay_thread = thread::spawn(move || {
            let current_process = unsafe { GetCurrentProcess() };
            let current_thread = unsafe { GetCurrentThread() };
            let mut real_thread_handle = INVALID_HANDLE_VALUE;
            unsafe {
                let _ = DuplicateHandle(
                    current_process,
                    current_thread,
                    current_process,
                    &mut real_thread_handle,
                    0,
                    false,
                    windows::Win32::Foundation::DUPLICATE_SAME_ACCESS,
                );
            }
            let _ = thread_handle_tx.send(real_thread_handle);

            let connect_result = unsafe {
                ConnectNamedPipe(relay_handle, ptr::null_mut())
            };
            if let Err(e) = connect_result {
                if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                    unsafe { let _ = CloseHandle(relay_handle); }
                    return;
                }
            }

            let pipe_file_read = unsafe {
                std::fs::File::from_raw_handle(relay_handle.0 as *mut _)
            };
            let pipe_file_write = match pipe_file_read.try_clone() {
                Ok(f) => f,
                Err(_) => {
                    return;
                }
            };

            let stdin_thread = thread::spawn(move || {
                if let Some(mut broker_stdin) = broker_stdin {
                    let mut pipe = pipe_file_read;
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

            let stdout_thread = thread::spawn(move || {
                if let Some(mut broker_stdout) = broker_stdout {
                    let mut pipe = pipe_file_write;
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

            stdout_thread.join().ok();
            drop(stdin_thread);
        });

        let thread_handle = thread_handle_rx.recv().unwrap_or(INVALID_HANDLE_VALUE);

        workers.push(RelayWorker {
            pipe_handle,
            relay_handle,
            thread_handle,
            relay_thread: Some(relay_thread),
            child,
            _pipe_security: security,
        });

        bridges.push(McpServerLaunch {
            name: server.name.clone(),
            command: bridge_command.clone(),
            args: vec!["--pipe".to_string(), pipe_path.clone()],
            env: Vec::new(),
        });
    }

    Ok((bridges, Some(McpBroker { workers })))
}

fn duplicate_handle(source: HANDLE) -> Result<HANDLE, String> {
    let current_process = unsafe { GetCurrentProcess() };
    let mut target: HANDLE = INVALID_HANDLE_VALUE;
    unsafe {
        DuplicateHandle(
            current_process,
            source,
            current_process,
            &mut target,
            0,
            false,
            windows::Win32::Foundation::DUPLICATE_SAME_ACCESS,
        )
        .map_err(|e| format!("DuplicateHandle: {e}"))?;
    }
    Ok(target)
}
