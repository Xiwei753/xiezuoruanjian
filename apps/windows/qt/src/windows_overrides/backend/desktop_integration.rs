//! 桌面外部集成（Windows）。
//!
//! 文本/终端操作只在这里处理 Windows 命令约定：
//! - `.exe` 直接作为终端子进程启动参数；
//! - `.cmd/.bat` 交给 PowerShell，用单引号转义后由 Windows shell 执行；
//! - 打开普通终端与 Agent 登录共用同一组终端启动 helper。

use cpp::cpp;
use qmetaobject::QString;
use std::path::{Path, PathBuf};
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

fn spawn(program: &str, args: &[&str], home: &Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

fn spawn_owned(program: &str, args: &[String], home: &Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

fn launch_plain_terminal(home: &Path) -> bool {
    let home_text = home.to_string_lossy().into_owned();
    if spawn("wt.exe", &["-d", &home_text], home) {
        return true;
    }
    // cmd start 会为新控制台窗口创建 powershell；GUI 进程直接 spawn
    // powershell 不一定有可见窗口。
    spawn(
        "cmd.exe",
        &["/D", "/C", "start", "", "powershell.exe"],
        home,
    )
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn powershell_invoke(path: &str, args: &[String]) -> String {
    let mut command = format!("& {}", powershell_quote(path));
    for arg in args {
        command.push(' ');
        command.push_str(&powershell_quote(arg));
    }
    command
}

fn is_batch_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

/// `.exe`：Windows Terminal 直接启动；失败时由 `cmd start` 建新控制台。
fn launch_exe_login(home: &Path, path: &str, args: &[String]) -> bool {
    let home_text = home.to_string_lossy().into_owned();
    let mut wt_args = vec!["-d".to_string(), home_text, path.to_string()];
    wt_args.extend(args.iter().cloned());
    if spawn_owned("wt.exe", &wt_args, home) {
        return true;
    }

    let mut start_args = vec![
        "/D".to_string(),
        "/C".to_string(),
        "start".to_string(),
        String::new(),
        path.to_string(),
    ];
    start_args.extend(args.iter().cloned());
    if spawn_owned("cmd.exe", &start_args, home) {
        return true;
    }

    let shell_command = powershell_invoke(path, args);
    spawn(
        "powershell.exe",
        &["-NoLogo", "-NoExit", "-Command", &shell_command],
        home,
    )
}

/// `.cmd/.bat`：不能直接 CreateProcess；统一由 PowerShell 解释并转义参数。
fn launch_batch_login(home: &Path, path: &str, args: &[String]) -> bool {
    let shell_command = powershell_invoke(path, args);
    let home_text = home.to_string_lossy().into_owned();
    let wt_args = vec![
        "-d".to_string(),
        home_text,
        "powershell.exe".to_string(),
        "-NoLogo".to_string(),
        "-NoExit".to_string(),
        "-Command".to_string(),
        shell_command.clone(),
    ];
    if spawn_owned("wt.exe", &wt_args, home) {
        return true;
    }
    spawn(
        "powershell.exe",
        &["-NoLogo", "-NoExit", "-Command", &shell_command],
        home,
    )
}

pub(crate) fn open_terminal() -> Result<(), String> {
    let Some(home) = user_home() else {
        return Err("无法确定用户 USERPROFILE，不能打开终端".to_string());
    };
    if launch_plain_terminal(&home) {
        Ok(())
    } else {
        Err("无法打开终端：未找到 Windows Terminal、PowerShell 或 cmd.exe".to_string())
    }
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
    let path = Path::new(launch_path);
    // 只确认终端包装器已启动；交互式 CLI 是否成功执行由终端内的用户可见结果决定。
    let launched = if is_batch_file(path) {
        launch_batch_login(&home, launch_path, login_args)
    } else {
        launch_exe_login(&home, launch_path, login_args)
    };
    if launched {
        Ok(())
    } else {
        Err("无法打开终端执行 Agent 登录".to_string())
    }
}
