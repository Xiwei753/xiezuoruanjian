//! Windows AppContainer 沙箱外的可信启动器。
//!
//! 接收 `--agent-id`、`--working-dir`、`--launch-path`、`--readable-dir`、
//! `--writable-dir` 和 `--` 后透传的 CLI 参数。在 AppContainer 中启动 CLI，
//! 转发 stdin/stdout/stderr，管理进程生命周期。
//!
//! 从宿主进程角度看，`sujian-agent-host.exe` 与直接启动 CLI 行为一致：
//! stdin/stdout/stderr 是标准管道，退出码反映 CLI 真实退出状态。

#[path = "../windows_overrides/agent_runtime/windows_sandbox.rs"]
mod windows_sandbox;

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::ptr;
use std::thread;

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

use windows_sandbox::{
    create_inheritable_pipe, grant_directory_access, make_non_inheritable,
    spawn_in_appcontainer, AppContainerProfile, SandboxCapabilities,
};

fn parse_args() -> Result<
    (
        String,
        PathBuf,
        String,
        Vec<PathBuf>,
        Vec<PathBuf>,
        Vec<String>,
    ),
    String,
> {
    let mut agent_id = String::new();
    let mut working_dir = PathBuf::new();
    let mut launch_path = String::new();
    let mut readable_dirs = Vec::new();
    let mut writable_dirs = Vec::new();
    let mut cli_args = Vec::new();
    let mut after_separator = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        if after_separator {
            cli_args.push(args[i].clone());
            i += 1;
            continue;
        }
        match args[i].as_str() {
            "--agent-id" => {
                i += 1;
                if i < args.len() {
                    agent_id = args[i].clone();
                }
            }
            "--working-dir" => {
                i += 1;
                if i < args.len() {
                    working_dir = PathBuf::from(&args[i]);
                }
            }
            "--launch-path" => {
                i += 1;
                if i < args.len() {
                    launch_path = args[i].clone();
                }
            }
            "--readable-dir" => {
                i += 1;
                if i < args.len() {
                    readable_dirs.push(PathBuf::from(&args[i]));
                }
            }
            "--writable-dir" => {
                i += 1;
                if i < args.len() {
                    writable_dirs.push(PathBuf::from(&args[i]));
                }
            }
            "--" => {
                after_separator = true;
            }
            _ => {
                return Err(format!("未知参数: {}", args[i]));
            }
        }
        i += 1;
    }

    if agent_id.is_empty() {
        return Err("--agent-id 是必需参数".to_string());
    }
    if working_dir.as_os_str().is_empty() {
        return Err("--working-dir 是必需参数".to_string());
    }
    if launch_path.is_empty() {
        return Err("--launch-path 是必需参数".to_string());
    }

    Ok((
        agent_id,
        working_dir,
        launch_path,
        readable_dirs,
        writable_dirs,
        cli_args,
    ))
}

fn main() {
    let (agent_id, working_dir, launch_path, readable_dirs, writable_dirs, cli_args) =
        match parse_args() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("sujian-agent-host: {e}");
                std::process::exit(2);
            }
        };

    let profile = match AppContainerProfile::for_agent(&agent_id) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建 AppContainer profile 失败: {e}");
            std::process::exit(3);
        }
    };

    if let Err(e) = grant_directory_access(&profile, &readable_dirs, &writable_dirs) {
        eprintln!("sujian-agent-host: 设置目录 ACL 失败: {e}");
        std::process::exit(4);
    }

    let capabilities = match SandboxCapabilities::new(&profile) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建沙箱 capabilities 失败: {e}");
            std::process::exit(5);
        }
    };

    let (stdin_read, stdin_write) = match create_inheritable_pipe() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建 stdin 管道失败: {e}");
            std::process::exit(6);
        }
    };
    let (stdout_read, stdout_write) = match create_inheritable_pipe() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建 stdout 管道失败: {e}");
            unsafe {
                let _ = CloseHandle(stdin_read);
                let _ = CloseHandle(stdin_write);
            }
            std::process::exit(6);
        }
    };
    let (stderr_read, stderr_write) = match create_inheritable_pipe() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建 stderr 管道失败: {e}");
            unsafe {
                let _ = CloseHandle(stdin_read);
                let _ = CloseHandle(stdin_write);
                let _ = CloseHandle(stdout_read);
                let _ = CloseHandle(stdout_write);
            }
            std::process::exit(6);
        }
    };

    if let Err(e) = make_non_inheritable(stdin_write) {
        eprintln!("sujian-agent-host: 设置 stdin_write 不可继承失败: {e}");
        unsafe {
            let _ = CloseHandle(stdin_read);
            let _ = CloseHandle(stdin_write);
            let _ = CloseHandle(stdout_read);
            let _ = CloseHandle(stdout_write);
            let _ = CloseHandle(stderr_read);
            let _ = CloseHandle(stderr_write);
        }
        std::process::exit(6);
    }
    if let Err(e) = make_non_inheritable(stdout_read) {
        eprintln!("sujian-agent-host: 设置 stdout_read 不可继承失败: {e}");
        unsafe {
            let _ = CloseHandle(stdin_read);
            let _ = CloseHandle(stdin_write);
            let _ = CloseHandle(stdout_read);
            let _ = CloseHandle(stdout_write);
            let _ = CloseHandle(stderr_read);
            let _ = CloseHandle(stderr_write);
        }
        std::process::exit(6);
    }
    if let Err(e) = make_non_inheritable(stderr_read) {
        eprintln!("sujian-agent-host: 设置 stderr_read 不可继承失败: {e}");
        unsafe {
            let _ = CloseHandle(stdin_read);
            let _ = CloseHandle(stdin_write);
            let _ = CloseHandle(stdout_read);
            let _ = CloseHandle(stdout_write);
            let _ = CloseHandle(stderr_read);
            let _ = CloseHandle(stderr_write);
        }
        std::process::exit(6);
    }

    let sandbox_process = match spawn_in_appcontainer(
        &launch_path,
        &cli_args,
        &working_dir,
        &capabilities,
        stdin_read,
        stdout_write,
        stderr_write,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sujian-agent-host: 启动沙箱进程失败: {e}");
            unsafe {
                let _ = CloseHandle(stdin_read);
                let _ = CloseHandle(stdin_write);
                let _ = CloseHandle(stdout_read);
                let _ = CloseHandle(stdout_write);
                let _ = CloseHandle(stderr_read);
                let _ = CloseHandle(stderr_write);
            }
            std::process::exit(7);
        }
    };

    unsafe {
        let _ = CloseHandle(stdin_read);
        let _ = CloseHandle(stdout_write);
        let _ = CloseHandle(stderr_write);
    }

    let stdin_handle = stdin_write;
    let stdout_handle = stdout_read;
    let stderr_handle = stderr_read;

    let stdin_handle_raw = stdin_handle.0 as usize;
    let stdin_thread = thread::spawn(move || {
        let stdin_handle = HANDLE(stdin_handle_raw as *mut _);
        let mut stdin = io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut offset = 0;
                    while offset < n {
                        let mut written: u32 = 0;
                        let write_result = unsafe {
                            WriteFile(
                                stdin_handle,
                                Some(&buf[offset..n]),
                                Some(&mut written),
                                None,
                            )
                        };
                        if write_result.is_err() || written == 0 {
                            break;
                        }
                        offset += written as usize;
                    }
                    if offset < n {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        unsafe { let _ = CloseHandle(stdin_handle) };
    });

    let stdout_handle_raw = stdout_handle.0 as usize;
    let stdout_thread = thread::spawn(move || {
        let stdout_handle = HANDLE(stdout_handle_raw as *mut _);
        let mut stdout = io::stdout();
        let mut buf = [0u8; 4096];
        loop {
            let mut read: u32 = 0;
            let result = unsafe {
                ReadFile(
                    stdout_handle,
                    Some(&mut buf),
                    Some(&mut read),
                    None,
                )
            };
            if result.is_err() || read == 0 {
                break;
            }
            if stdout.write_all(&buf[..read as usize]).is_err() {
                break;
            }
            let _ = stdout.flush();
        }
        unsafe { let _ = CloseHandle(stdout_handle) };
    });

    let stderr_handle_raw = stderr_handle.0 as usize;
    let stderr_thread = thread::spawn(move || {
        let stderr_handle = HANDLE(stderr_handle_raw as *mut _);
        let mut stderr = io::stderr();
        let mut buf = [0u8; 4096];
        loop {
            let mut read: u32 = 0;
            let result = unsafe {
                ReadFile(
                    stderr_handle,
                    Some(&mut buf),
                    Some(&mut read),
                    None,
                )
            };
            if result.is_err() || read == 0 {
                break;
            }
            let _ = stderr.write_all(&buf[..read as usize]);
        }
        unsafe { let _ = CloseHandle(stderr_handle) };
    });

    unsafe {
        let result = WaitForSingleObject(sandbox_process.handle, u32::MAX);
        let exit_code = if result == WAIT_OBJECT_0 {
            let mut code: u32 = 0;
            GetExitCodeProcess(sandbox_process.handle, &mut code).ok();
            code
        } else {
            1
        };

        drop(sandbox_process);

        stdout_thread.join().ok();
        stderr_thread.join().ok();

        std::process::exit(exit_code as i32);
    }
}
