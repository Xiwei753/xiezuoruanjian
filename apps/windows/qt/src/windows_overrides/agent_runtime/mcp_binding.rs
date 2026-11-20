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
//!
//! 所有 pipe 使用 `FILE_FLAG_OVERLAPPED` 创建，`ConnectNamedPipe` 和
//! 数据转发均使用 overlapped I/O + event，`McpBroker::Drop` 通过
//! `SetEvent(stop_event)` + `CancelIoEx` 可靠取消所有未完成的 I/O。

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::thread::{self, JoinHandle};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
    ERROR_PIPE_CONNECTED, WAIT_OBJECT_0, WAIT_FAILED, GetLastError,
};
use windows::Win32::Security::{
    InitializeSecurityDescriptor, ACL, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    SetSecurityDescriptorDacl, SECURITY_DESCRIPTOR,
};
use windows::Win32::Security::Authorization::{
    SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_GROUP,
    TRUSTEE_W,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE,
    PIPE_READMODE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, SetEvent, WaitForMultipleObjects,
};
use windows::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_FLAG_OVERLAPPED,
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
struct PipeSecurity {
    sd: Box<SECURITY_DESCRIPTOR>,
    acl: *mut ACL,
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self.acl.is_null() {
            unsafe { let _ = windows::Win32::Foundation::LocalFree(HLOCAL(self.acl.cast())); }
        }
    }
}

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

    let mut new_acl: *mut ACL = ptr::null_mut();
    unsafe {
        let result = SetEntriesInAclW(Some(&[explicit_access]), None, &mut new_acl);
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
            Some(new_acl as *const ACL),
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

fn create_event() -> Result<HANDLE, String> {
    unsafe {
        CreateEventW(None, true, false, PCWSTR(ptr::null()))
            .map_err(|e| format!("CreateEventW: {e}"))
    }
}

/// 单个 MCP relay 的所有资源。
///
/// pipe 使用 `FILE_FLAG_OVERLAPPED` 创建，所有 I/O 使用 overlapped + event。
/// `stop_event` 用于通知 relay 线程停止；`SetEvent(stop_event)` + `CancelIoEx`
/// 可靠取消所有未完成的 I/O。
struct RelayWorker {
    pipe_handle: HANDLE,
    stop_event: HANDLE,
    relay_thread: Option<JoinHandle<()>>,
    child: Child,
    _pipe_security: PipeSecurity,
}

impl Drop for RelayWorker {
    fn drop(&mut self) {
        unsafe { let _ = SetEvent(self.stop_event); }
        let _ = self.child.kill();
        let _ = self.child.wait();
        unsafe { let _ = CancelIoEx(self.pipe_handle, None); }
        if let Some(thread) = self.relay_thread.take() {
            let _ = thread.join();
        }
        unsafe {
            let _ = CloseHandle(self.stop_event);
            let _ = CloseHandle(self.pipe_handle);
        }
    }
}

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
        // RelayWorker::drop handles shutdown + close_handles for each worker
    }
}

/// 把当前会话的 MCP 配置拆成"沙箱外 broker + 沙箱内 named pipe bridge"。
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
    let mut workers: Vec<RelayWorker> = Vec::new();

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
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
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
            // workers' Drop will clean up previously created resources
            return Err(format!("CreateNamedPipeW 失败: {}", pipe_path));
        }

        let mut broker_cmd = Command::new(&server.command);
        broker_cmd
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .envs(server.env.iter().cloned());

        let mut child = match broker_cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                drop(security);
                unsafe { let _ = CloseHandle(pipe_handle); }
                return Err(format!("无法启动作品 MCP broker（{}）：{e}", server.command));
            }
        };

        let broker_stdin = child.stdin.take();
        let broker_stdout = child.stdout.take();

        let stop_event = match create_event() {
            Ok(h) => h,
            Err(e) => {
                drop(security);
                let _ = child.kill();
                let _ = child.wait();
                unsafe { let _ = CloseHandle(pipe_handle); }
                return Err(e);
            }
        };

        let pipe_handle_raw = pipe_handle.0 as usize;
        let stop_event_raw = stop_event.0 as usize;
        let relay_thread = thread::spawn(move || {
            let pipe_handle = HANDLE(pipe_handle_raw as *mut _);
            let stop_event = HANDLE(stop_event_raw as *mut _);
            relay_main(pipe_handle, stop_event, broker_stdin, broker_stdout);
        });

        workers.push(RelayWorker {
            pipe_handle,
            stop_event,
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

/// relay 线程主逻辑：overlapped ConnectNamedPipe + 双向转发。
fn relay_main(
    pipe_handle: HANDLE,
    stop_event: HANDLE,
    broker_stdin: Option<std::process::ChildStdin>,
    broker_stdout: Option<std::process::ChildStdout>,
) {
    let connect_event = match create_event() {
        Ok(h) => h,
        Err(_) => return,
    };

    let mut connect_overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    connect_overlapped.hEvent = connect_event;

    let connect_result = unsafe {
        ConnectNamedPipe(pipe_handle, Some(&mut connect_overlapped))
    };

    // Classify ConnectNamedPipe return status
    let connected = if connect_result.is_ok() {
        // Immediate success — connection completed synchronously
        true
    } else {
        let err = unsafe { GetLastError() };
        if err == ERROR_PIPE_CONNECTED {
            // Client already connected between CreateNamedPipeW and ConnectNamedPipe.
            // SetEvent so WaitForMultipleObjects can uniformly detect completion.
            unsafe { let _ = SetEvent(connect_event); }
            true
        } else if err == windows::Win32::Foundation::ERROR_IO_PENDING {
            // Pending — wait for connect_event or stop_event
            let wait_result = unsafe {
                WaitForMultipleObjects(
                    &[connect_event, stop_event],
                    false,
                    windows::Win32::System::Threading::INFINITE,
                )
            };
            let wait_val = wait_result.0;
            if wait_val == WAIT_FAILED.0 {
                // Wait failed — cancel and exit
                unsafe {
                    let _ = CancelIoEx(pipe_handle, Some(&mut connect_overlapped));
                    let mut dummy: u32 = 0;
                    let _ = GetOverlappedResult(pipe_handle, &mut connect_overlapped, &mut dummy, true);
                }
                unsafe { let _ = CloseHandle(connect_event); }
                return;
            } else if wait_val == WAIT_OBJECT_0.0 + 1 {
                // stop_event signaled — cancel and exit
                unsafe {
                    let _ = CancelIoEx(pipe_handle, Some(&mut connect_overlapped));
                    let mut dummy: u32 = 0;
                    let _ = GetOverlappedResult(pipe_handle, &mut connect_overlapped, &mut dummy, true);
                }
                unsafe { let _ = CloseHandle(connect_event); }
                return;
            } else if wait_val == WAIT_OBJECT_0.0 {
                // connect_event signaled — verify overlapped result
                let mut bytes_transferred: u32 = 0;
                let overlapped_result = unsafe {
                    GetOverlappedResult(pipe_handle, &mut connect_overlapped, &mut bytes_transferred, true)
                };
                overlapped_result.is_ok()
            } else {
                // Unexpected wait result
                unsafe { let _ = CloseHandle(connect_event); }
                return;
            }
        } else {
            // Other error — exit
            unsafe { let _ = CloseHandle(connect_event); }
            return;
        }
    };

    if !connected {
        unsafe { let _ = CloseHandle(connect_event); }
        return;
    }

    unsafe { let _ = CloseHandle(connect_event); }

    // Bidirectional forwarding with two sub-threads
    let pipe_handle_raw = pipe_handle.0 as usize;
    let stop_event_raw = stop_event.0 as usize;

    let stdin_thread = thread::spawn(move || {
        let pipe_handle = HANDLE(pipe_handle_raw as *mut _);
        let stop_event = HANDLE(stop_event_raw as *mut _);
        if let Some(mut broker_stdin) = broker_stdin {
            let mut buf = [0u8; 4096];
            loop {
                let read_event = match create_event() {
                    Ok(h) => h,
                    Err(_) => break,
                };
                let mut read_overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
                read_overlapped.hEvent = read_event;

                let mut bytes_read: u32 = 0;
                let read_result = unsafe {
                    ReadFile(pipe_handle, Some(&mut buf), Some(&mut bytes_read), Some(&mut read_overlapped))
                };

                if read_result.is_ok() {
                    // Immediate completion
                    unsafe { let _ = CloseHandle(read_event); }
                    if bytes_read == 0 {
                        break;
                    }
                    if broker_stdin.write_all(&buf[..bytes_read as usize]).is_err() {
                        break;
                    }
                    let _ = broker_stdin.flush();
                    continue;
                }

                let err = unsafe { GetLastError() };
                if err != windows::Win32::Foundation::ERROR_IO_PENDING {
                    unsafe { let _ = CloseHandle(read_event); }
                    break;
                }

                // Pending — wait for read_event or stop_event
                let wait_result = unsafe {
                    WaitForMultipleObjects(
                        &[read_event, stop_event],
                        false,
                        windows::Win32::System::Threading::INFINITE,
                    )
                };
                let wait_val = wait_result.0;

                if wait_val == WAIT_FAILED.0 || wait_val == WAIT_OBJECT_0.0 + 1 {
                    // stop_event or wait failed — cancel and exit
                    unsafe {
                        let _ = CancelIoEx(pipe_handle, Some(&mut read_overlapped));
                        let mut dummy: u32 = 0;
                        let _ = GetOverlappedResult(pipe_handle, &mut read_overlapped, &mut dummy, true);
                    }
                    unsafe { let _ = CloseHandle(read_event); }
                    break;
                }

                // read_event signaled — get result
                let mut transferred: u32 = 0;
                let overlapped_result = unsafe {
                    GetOverlappedResult(pipe_handle, &mut read_overlapped, &mut transferred, true)
                };

                unsafe { let _ = CloseHandle(read_event); }

                if overlapped_result.is_err() || transferred == 0 {
                    break;
                }

                if broker_stdin.write_all(&buf[..transferred as usize]).is_err() {
                    break;
                }
                let _ = broker_stdin.flush();
            }
        }
    });

    let stdout_thread = thread::spawn(move || {
        let pipe_handle = HANDLE(pipe_handle_raw as *mut _);
        let stop_event = HANDLE(stop_event_raw as *mut _);
        if let Some(mut broker_stdout) = broker_stdout {
            let mut buf = [0u8; 4096];
            loop {
                let n = match broker_stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let mut offset = 0;
                while offset < n {
                    let write_event = match create_event() {
                        Ok(h) => h,
                        Err(_) => return,
                    };

                    let mut write_overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
                    write_overlapped.hEvent = write_event;

                    let mut bytes_written: u32 = 0;
                    let write_result = unsafe {
                        WriteFile(pipe_handle, Some(&buf[offset..n]), Some(&mut bytes_written), Some(&mut write_overlapped))
                    };

                    if write_result.is_ok() {
                        unsafe { let _ = CloseHandle(write_event); }
                        if bytes_written == 0 {
                            return;
                        }
                        offset += bytes_written as usize;
                        continue;
                    }

                    let err = unsafe { GetLastError() };
                    if err != windows::Win32::Foundation::ERROR_IO_PENDING {
                        unsafe { let _ = CloseHandle(write_event); }
                        return;
                    }

                    let wait_result = unsafe {
                        WaitForMultipleObjects(
                            &[write_event, stop_event],
                            false,
                            windows::Win32::System::Threading::INFINITE,
                        )
                    };
                    let wait_val = wait_result.0;

                    if wait_val == WAIT_FAILED.0 || wait_val == WAIT_OBJECT_0.0 + 1 {
                        unsafe {
                            let _ = CancelIoEx(pipe_handle, Some(&mut write_overlapped));
                            let mut dummy: u32 = 0;
                            let _ = GetOverlappedResult(pipe_handle, &mut write_overlapped, &mut dummy, true);
                        }
                        unsafe { let _ = CloseHandle(write_event); }
                        return;
                    }

                    let mut transferred: u32 = 0;
                    let overlapped_result = unsafe {
                        GetOverlappedResult(pipe_handle, &mut write_overlapped, &mut transferred, true)
                    };

                    unsafe { let _ = CloseHandle(write_event); }

                    if overlapped_result.is_err() || transferred == 0 {
                        return;
                    }
                    offset += transferred as usize;
                }
            }
        }
    });

    let _ = stdin_thread.join();
    let _ = stdout_thread.join();
}
