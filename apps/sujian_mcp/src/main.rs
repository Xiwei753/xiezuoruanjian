mod context;
mod server;
mod tools;

use std::path::PathBuf;

use crate::context::McpContext;
use crate::server::SujianMcpServer;
use rmcp::transport::stdio;

fn usage() -> &'static str {
    "sujian_mcp — local MCP server for Sujian\n\
     \n\
     Usage: sujian_mcp [--app-data-root <DIR>] [--projects-root <DIR>]\n\
     \n\
     Options:\n\
       --app-data-root <DIR>  Sujian application data root (default: $SUJIAN_APP_DATA_ROOT,\n\
                              then $XDG_DATA_HOME/sujian or ~/.local/share/sujian)\n\
       --projects-root <DIR>  Projects/workspace root (default: $SUJIAN_PROJECTS_ROOT,\n\
                              then <app-data-root>/projects)\n\
       -h, --help             Show this help\n\
     \n\
     The server speaks MCP over stdio; diagnostics go to stderr."
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

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut app_data_root = None;
    let mut projects_root = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--app-data-root" => {
                app_data_root = Some(PathBuf::from(next_value(&mut args, &arg)?));
            }
            "--projects-root" => {
                projects_root = Some(PathBuf::from(next_value(&mut args, &arg)?));
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
    Ok((app_data_root, projects_root))
}

#[tokio::main]
async fn main() {
    let (app_data_root, projects_root) = match parse_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("sujian_mcp: {error}\n\n{}", usage());
            std::process::exit(2);
        }
    };

    eprintln!(
        "sujian_mcp: app data root = {}\nsujian_mcp: projects root = {}",
        app_data_root.display(),
        projects_root.display()
    );

    let server = SujianMcpServer::new(McpContext::new(&app_data_root, &projects_root));

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
