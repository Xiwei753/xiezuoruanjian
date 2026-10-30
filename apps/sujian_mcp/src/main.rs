mod context;
mod server;
mod tools;

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::context::McpContext;
use crate::server::SujianMcpServer;
use rmcp::transport::stdio;

#[derive(Debug)]
enum Mode {
    Stdio,
    /// 沙箱外运行的 broker：监听 Unix socket，接受沙箱内 bridge 的连接。
    UnixSocket(PathBuf),
    /// 沙箱内运行的 stdio 桥：把 stdin/stdout 转发到沙箱外 broker。
    StdioBridge(PathBuf),
}

fn usage() -> &'static str {
    "sujian_mcp — local MCP server for Sujian\n\
     \n\
     Usage: sujian_mcp [--app-data-root <DIR>] [--projects-root <DIR>]\n\
            sujian_mcp --unix-socket <PATH> [--app-data-root <DIR>] [--projects-root <DIR>]\n\
            sujian_mcp --stdio-bridge <PATH>\n\
     \n\
     Options:\n\
       --app-data-root <DIR>  Sujian application data root (default: $SUJIAN_APP_DATA_ROOT,\n\
                              then $XDG_DATA_HOME/sujian or ~/.local/share/sujian)\n\
       --projects-root <DIR>  Projects/workspace root (default: $SUJIAN_PROJECTS_ROOT,\n\
                              then <app-data-root>/projects)\n\
       --unix-socket <PATH>   Serve MCP over a Unix socket (used outside the Agent sandbox)\n\
       --stdio-bridge <PATH>  Bridge stdio to a Unix-socket MCP broker (used inside sandbox)\n\
       -h, --help             Show this help\n\
     \n\
     Without --unix-socket/--stdio-bridge the server speaks MCP over stdio;\n\
     diagnostics go to stderr."
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn default_app_data_root() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("SUJIAN_APP_DATA_ROOT") {
        return Ok(PathBuf::from(value));
    }
    if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(value).join("sujian"));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/share/sujian"))
        .ok_or_else(|| "cannot determine app data root; pass --app-data-root".to_string())
}

fn parse_args() -> Result<(PathBuf, PathBuf, Mode), String> {
    let mut app_data_root = None;
    let mut projects_root = None;
    let mut unix_socket = None;
    let mut stdio_bridge = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--app-data-root" => {
                app_data_root = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "--projects-root" => {
                projects_root = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "--unix-socket" => {
                unix_socket = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "--stdio-bridge" => {
                stdio_bridge = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "-h" | "--help" => {
                eprintln!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let app_data_root = match app_data_root {
        Some(value) => value,
        None => default_app_data_root()?,
    };
    let projects_root = projects_root
        .or_else(|| std::env::var_os("SUJIAN_PROJECTS_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| app_data_root.join("projects"));
    let mode = if let Some(path) = stdio_bridge {
        if unix_socket.is_some() {
            return Err("--stdio-bridge 与 --unix-socket 不能同时使用".to_string());
        }
        Mode::StdioBridge(path)
    } else if let Some(path) = unix_socket {
        Mode::UnixSocket(path)
    } else {
        Mode::Stdio
    };
    Ok((app_data_root, projects_root, mode))
}

fn run_stdio_bridge(path: &Path) -> Result<(), String> {
    let mut stream = std::os::unix::net::UnixStream::connect(path)
        .map_err(|error| format!("连接 MCP broker {} 失败：{error}", path.display()))?;
    let mut to_socket = stream
        .try_clone()
        .map_err(|error| format!("复制 MCP bridge socket 失败：{error}"))?;
    std::thread::Builder::new()
        .name("sujian-mcp-bridge-stdin".to_string())
        .spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buffer = [0_u8; 8192];
            loop {
                match stdin.read(&mut buffer) {
                    Ok(0) | Err(_) => {
                        let _ = to_socket.shutdown(std::net::Shutdown::Write);
                        return;
                    }
                    Ok(read) => {
                        if to_socket.write_all(&buffer[..read]).is_err() {
                            return;
                        }
                    }
                }
            }
        })
        .map_err(|error| format!("启动 MCP bridge stdin 线程失败：{error}"))?;
    let mut stdout = std::io::stdout();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("读取 MCP broker 失败：{error}"))?;
        if read == 0 {
            return Ok(());
        }
        stdout
            .write_all(&buffer[..read])
            .and_then(|_| stdout.flush())
            .map_err(|error| format!("写入 MCP bridge stdout 失败：{error}"))?;
    }
}

async fn serve_unix(path: PathBuf, server: SujianMcpServer) {
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("sujian_mcp: remove stale socket failed: {error}");
            std::process::exit(1);
        }
    }
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("sujian_mcp: bind {} failed: {error}", path.display());
            std::process::exit(1);
        }
    };
    if let Err(error) =
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
    {
        eprintln!("sujian_mcp: chmod socket failed: {error}");
        std::process::exit(1);
    }
    eprintln!("sujian_mcp: MCP broker listening on {}", path.display());
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("sujian_mcp: accept failed: {error}");
                continue;
            }
        };
        let server = server.clone();
        tokio::spawn(async move {
            match rmcp::serve_server(server, stream).await {
                Ok(service) => {
                    if let Err(error) = service.waiting().await {
                        eprintln!("sujian_mcp: broker session failed: {error}");
                    }
                }
                Err(error) => eprintln!("sujian_mcp: broker start failed: {error}"),
            }
        });
    }
}

#[tokio::main]
async fn main() {
    let (app_data_root, projects_root, mode) = match parse_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("sujian_mcp: {error}\n\n{}", usage());
            std::process::exit(2);
        }
    };

    if let Mode::StdioBridge(path) = &mode {
        if let Err(error) = run_stdio_bridge(path) {
            eprintln!("sujian_mcp: {error}");
            std::process::exit(1);
        }
        return;
    }

    eprintln!(
        "sujian_mcp: app data root = {}\nsujian_mcp: projects root = {}",
        app_data_root.display(),
        projects_root.display()
    );

    let server = SujianMcpServer::new(McpContext::new(&app_data_root, &projects_root));

    if let Mode::UnixSocket(path) = mode {
        serve_unix(path, server).await;
        return;
    }

    match rmcp::serve_server(server, stdio()).await {
        Ok(service) => {
            if let Err(error) = service.waiting().await {
                eprintln!("sujian_mcp: server task failed: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("sujian_mcp: failed to start MCP server: {error}");
            std::process::exit(1);
        }
    }
}
