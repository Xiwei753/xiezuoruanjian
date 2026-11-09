//! 桌面外部集成（Windows）。
//!
//! 使用 Windows Terminal / PowerShell / cmd 打开用户 HOME 下的终端；
//! 文档 URL 交给 `QDesktopServices`，由系统默认浏览器处理。

use cpp::cpp;
use qmetaobject::QString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

cpp! {{
    #include <QDesktopServices>
    #include <QString>
    #include <QUrl>
}}

pub(crate) fn user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn spawn(program: &str, args: &[&str], home: &std::path::Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// 打开用户 HOME 下的终端，不把当前作品目录当成外部 CLI 的工作目录。
pub(crate) fn open_terminal() -> Result<(), String> {
    let Some(home) = user_home() else {
        return Err("无法确定用户 USERPROFILE，不能打开终端".to_string());
    };
    let home_text = home.to_string_lossy().into_owned();
    if spawn("wt.exe", &["-d", &home_text], &home) {
        return Ok(());
    }
    if spawn("cmd.exe", &["/C", "start", "", "powershell.exe"], &home) {
        return Ok(());
    }
    if spawn("cmd.exe", &["/C", "start", "", "cmd.exe"], &home) {
        return Ok(());
    }
    Err("无法打开终端：未找到 Windows Terminal、PowerShell 或 cmd.exe".to_string())
}

pub(crate) fn open_url(url: &str) -> Result<(), String> {
    let qurl = QString::from(url);
    let opened = cpp!(unsafe [qurl as "QString"] -> bool as "bool" {
        return QDesktopServices::openUrl(QUrl(qurl));
    });
    if opened {
        Ok(())
    } else {
        Err("无法打开文档：Windows 默认浏览器拒绝了该 URL".to_string())
    }
}

pub(crate) fn open_agent_login(launch_path: &str, login_args: &[String]) -> Result<(), String> {
    let Some(home) = user_home() else {
        return Err("无法确定用户 USERPROFILE，不能打开终端".to_string());
    };
    let home_text = home.to_string_lossy().into_owned();
    let mut wt_args = vec!["-d".to_string(), home_text.clone(), launch_path.to_string()];
    wt_args.extend(login_args.iter().cloned());
    if spawn_owned("wt.exe", &wt_args, &home) {
        return Ok(());
    }

    let mut cmd_args = vec![
        "/C".to_string(),
        "start".to_string(),
        String::new(),
        launch_path.to_string(),
    ];
    cmd_args.extend(login_args.iter().cloned());
    if spawn_owned("cmd.exe", &cmd_args, &home) {
        return Ok(());
    }
    Err("无法打开终端执行 Agent 登录".to_string())
}

fn spawn_owned(program: &str, args: &[String], home: &std::path::Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}
