// =============================================================================
// probe.rs — 仅对已匹配 Agent 做受限探测
// =============================================================================
//
// Issue #1014：绝不执行任意 PATH 程序的 `--version`，也不读取凭据文件。
// 只有描述里明确声明了 version_args 的 Agent 才会运行；超时必须杀掉进程。
// 登录状态无法从 CLI 自身确认时保持 unknown，不猜测已登录。
// =============================================================================

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::inventory::{AgentDescriptor, AgentProbe, DiscoveredExecutable};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const OUTPUT_LIMIT: usize = 200;

pub(crate) fn run_with_timeout<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|error| error.to_string());
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("超时（{} 秒）", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

pub fn probe_executable(
    executable: &DiscoveredExecutable,
    descriptor: &AgentDescriptor,
) -> AgentProbe {
    let mut probe = AgentProbe::default();
    let path = Path::new(&executable.launch_path);
    let launchable = path.is_file()
        && path
            .metadata()
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false);
    probe.launchability = if launchable {
        "launchable"
    } else {
        "unavailable"
    }
    .to_string();

    if !launchable {
        probe.compatibility = "unsupported".to_string();
        probe.error = Some("可执行文件不存在或不可执行".to_string());
        return probe;
    }

    probe.config_status = config_status(&descriptor.config_paths);
    let requires_opencode_v2 = descriptor.id == "opencode";
    if !requires_opencode_v2 {
        // Codex 等适配器保留自己的版本规则；这里只表示可执行文件可启动。
        probe.compatibility = "supported".to_string();
    }

    if descriptor.version_args.is_empty() {
        if requires_opencode_v2 {
            mark_opencode_incompatible(&mut probe, executable, None);
        }
        return probe;
    }

    match run_with_timeout(
        &executable.launch_path,
        &descriptor.version_args,
        PROBE_TIMEOUT,
    ) {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if let Some(version) =
                first_non_empty_line(&stdout).or_else(|| first_non_empty_line(&stderr))
            {
                probe.version = Some(version);
                probe.version_status = "detected".to_string();
            } else {
                probe.version_status = "empty".to_string();
            }
        }
        Ok(_) => {
            probe.version_status = "failed".to_string();
        }
        Err(error) => {
            probe.version_status = "failed".to_string();
            probe.error = Some(error);
        }
    }

    if requires_opencode_v2 {
        let major = probe.version.as_deref().and_then(parse_major);
        if major == Some(2) {
            probe.compatibility = "supported".to_string();
        } else {
            mark_opencode_incompatible(&mut probe, executable, major);
        }
    }

    probe
}

/// 从 `opencode --version` 等输出中提取主版本。允许程序名前缀（如
/// `opencode 2.0.1`）、`v` 前缀、预发布/修订后缀；只取第一个带数字的
/// 版本 token，避免把路径或说明文字误当成版本。
fn parse_major(version: &str) -> Option<u64> {
    for token in version.split_whitespace() {
        let token = token.trim_matches(|value: char| {
            matches!(value, ',' | ';' | '(' | ')' | '[' | ']' | ':' | '"' | '\'')
        });
        let token = token
            .strip_prefix('v')
            .or_else(|| token.strip_prefix('V'))
            .unwrap_or(token);
        let digits: String = token
            .chars()
            .take_while(|value| value.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

fn mark_opencode_incompatible(
    probe: &mut AgentProbe,
    executable: &DiscoveredExecutable,
    major: Option<u64>,
) {
    probe.compatibility = "unsupported".to_string();
    let path = &executable.launch_path;
    let reason = match (&probe.version, probe.version_status.as_str()) {
        (Some(version), "detected") if major.is_some() => format!(
            "检测到 OpenCode {version}（主版本 {}），素笺只支持 OpenCode 2.x",
            major.unwrap_or_default()
        ),
        (Some(version), "detected") => {
            format!("无法从版本输出解析主版本：{version}；素笺需要 OpenCode 2.x")
        }
        (_, "empty") => "版本输出为空，无法确认 OpenCode 2.x".to_string(),
        (_, "failed") => {
            let detail = probe
                .error
                .take()
                .unwrap_or_else(|| "版本探测失败".to_string());
            format!("版本探测失败（{detail}），无法确认 OpenCode 2.x")
        }
        _ => "无法确认 OpenCode 2.x".to_string(),
    };
    probe.error = Some(format!("{reason}（路径：{path}）"));
}

fn first_non_empty_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(OUTPUT_LIMIT).collect())
}

fn config_status(paths: &[String]) -> String {
    if paths.is_empty() {
        return "unknown".to_string();
    }
    if paths.iter().any(|path| expand(path).exists()) {
        "configured".to_string()
    } else {
        "not-configured".to_string()
    }
}

pub(crate) fn expand(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home);
        }
    }
    std::path::PathBuf::from(path)
}
