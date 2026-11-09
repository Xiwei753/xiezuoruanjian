//! 桌面外部集成（Linux）。
//!
//! 只处理“打开终端 / 打开文档 URL / 在终端执行 Agent 登录”这类系统动作，
//! 不承载 Agent 发现、选择或任何正文业务。Windows 用同名模块覆盖实现。

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

/// 打开一个位于用户 HOME 的普通终端。外部 TUI 由用户自行启动，
/// 素笺不注入工作区路径、不执行 OpenCode/Codex，也不创建新会话。
pub(crate) fn open_terminal() -> Result<(), String> {
    let Some(home) = user_home() else {
        return Err("无法确定用户 HOME，不能打开终端".to_string());
    };
    for program in [
        "xdg-terminal-exec",
        "konsole",
        "gnome-terminal",
        "kgx",
        "x-terminal-emulator",
        "kitty",
        "alacritty",
        "foot",
        "wezterm",
        "xterm",
    ] {
        let mut command = Command::new(program);
        command
            .current_dir(&home)
            .env("PWD", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if command.spawn().is_ok() {
            return Ok(());
        }
    }
    Err(
        "无法打开终端：未找到可用的终端程序（xdg-terminal-exec、konsole、gnome-terminal、kgx、x-terminal-emulator、kitty、alacritty、foot、wezterm 或 xterm）"
            .to_string(),
    )
}

pub(crate) fn open_url(url: &str) -> Result<(), String> {
    let attempts = vec![
        ("xdg-open", vec![url.to_string()]),
        ("gio", vec!["open".to_string(), url.to_string()]),
    ];
    for (program, args) in attempts {
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return Ok(());
        }
    }
    Err("无法打开文档：未找到 xdg-open 或 gio".to_string())
}

fn terminal_attempts() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("xdg-terminal-exec", Vec::new()),
        ("konsole", vec!["-e".to_string()]),
        ("gnome-terminal", vec!["--".to_string()]),
        ("x-terminal-emulator", vec!["-e".to_string()]),
        ("kgx", vec!["--".to_string()]),
        ("xterm", vec!["-e".to_string()]),
    ]
}

pub(crate) fn open_agent_login(launch_path: &str, login_args: &[String]) -> Result<(), String> {
    let Some(home) = user_home() else {
        return Err("无法确定用户 HOME，不能打开终端".to_string());
    };
    for (program, terminal_args) in terminal_attempts() {
        let mut command = Command::new(program);
        command
            .args(terminal_args)
            .arg(launch_path)
            .args(login_args)
            .current_dir(&home)
            .env("PWD", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if command.spawn().is_ok() {
            return Ok(());
        }
    }
    Err("无法打开终端执行 Agent 登录".to_string())
}
