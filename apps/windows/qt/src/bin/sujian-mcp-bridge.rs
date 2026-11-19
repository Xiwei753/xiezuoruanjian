//! 沙箱内 CLI 的 MCP stdio↔named-pipe 桥。
//!
//! Agent CLI 在 AppContainer 沙箱中运行，按 MCP stdio 约定启动本程序作为
//! MCP server。本程序连接到沙箱外可信进程创建的 named pipe，把 stdio
//! 上的 JSON-RPC 流双向转发到 pipe，使沙箱内的 CLI 通过 pipe 间接访问
//! 沙箱外的 `sujian_mcp.exe` broker。

use std::io::{self, Read, Write};
use std::os::windows::io::FromRawHandle;
use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_NONE, OPEN_EXISTING,
};

fn wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn main() {
    let mut pipe_path = String::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pipe" => {
                i += 1;
                if i < args.len() {
                    pipe_path = args[i].clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    if pipe_path.is_empty() {
        eprintln!("sujian-mcp-bridge: --pipe 是必需参数");
        std::process::exit(2);
    }

    let pipe_wide = wide(&pipe_path);
    let pipe_handle = unsafe {
        CreateFileW(
            PCWSTR(pipe_wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            0,
            None,
        )
    };

    if pipe_handle.is_invalid() {
        eprintln!("sujian-mcp-bridge: 无法连接 named pipe: {pipe_path}");
        std::process::exit(3);
    }

    let pipe_file = unsafe { std::fs::File::from_raw_handle(pipe_handle.0 as *mut _) };
    let pipe_write = pipe_file.try_clone().expect("clone pipe handle");

    let stdin_thread = thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 4096];
        let mut pipe = pipe_write;
        loop {
            match stdin.read(&mut buf) {
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
    });

    let stdout_thread = thread::spawn(move || {
        let mut stdout = io::stdout();
        let mut buf = [0u8; 4096];
        let mut pipe = pipe_file;
        loop {
            match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if stdout.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
    });

    stdout_thread.join().ok();

    std::process::exit(0);
}
