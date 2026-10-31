//! Side-loaded Agent 的 Linux 文件系统边界。
//!
//! Agent 只能看到运行所需的系统目录、当前作品（只读）、自身认证/状态目录
//! （可写）和 Sujian MCP broker 的 Unix Socket。宿主 `/run/user`、D-Bus、
//! systemd user manager、X11/Wayland 等会话 IPC 不进入沙箱；进程环境继承
//! 用户启动 CLI 时的配置与认证变量，但排除这些会话 IPC 入口，避免通过
//! `systemd-run --user` 等方式绕过 Core 直接改作品。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::events::AgentLaunchContext;

pub struct WorkspaceAccess {
    bwrap: PathBuf,
    home: PathBuf,
    agent_id: String,
    projects_root: PathBuf,
    working_dir: PathBuf,
    mcp_socket_dir: Option<PathBuf>,
    extra_ro_paths: Vec<PathBuf>,
}

fn executable(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resolve_bwrap() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("SUJIAN_BWRAP") {
        let path = PathBuf::from(value);
        if executable(&path) {
            return Ok(path);
        }
        return Err(format!(
            "SUJIAN_BWRAP 指向的程序不可执行：{}",
            path.display()
        ));
    }
    for candidate in ["/usr/bin/bwrap", "/usr/local/bin/bwrap"] {
        let path = PathBuf::from(candidate);
        if executable(&path) {
            return Ok(path);
        }
    }
    find_in_path("bwrap").ok_or_else(|| {
        "未找到 bubblewrap（bwrap）。侧载 Agent 必须在只读作品沙箱中运行，请先安装 bubblewrap"
            .to_string()
    })
}

fn agent_state_dirs(
    agent_id: &str,
    home: &Path,
    config_home: &Path,
    data_home: &Path,
    state_home: &Path,
    cache_home: &Path,
) -> Vec<PathBuf> {
    let names: Vec<&str> = match agent_id {
        "codex" => vec!["codex"],
        "opencode" => vec!["opencode"],
        _ => Vec::new(),
    };

    let mut dirs = Vec::new();
    for name in names {
        // HOME 下的点目录是多数 CLI 的兼容位置；XDG 基目录则尊重用户
        // 启动 OpenCode/Codex 时的真实配置环境，两边都保留。
        dirs.push(home.join(format!(".{name}")));
        dirs.push(home.join(".config").join(name));
        dirs.push(home.join(".local/share").join(name));
        dirs.push(home.join(".local/state").join(name));
        dirs.push(home.join(".cache").join(name));
        dirs.push(config_home.join(name));
        dirs.push(data_home.join(name));
        dirs.push(state_home.join(name));
        dirs.push(cache_home.join(name));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn env_absolute_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// 沙箱继承宿主环境中的配置和认证变量（包括各 CLI 自己的 API Key），
/// 但不继承桌面会话、D-Bus、SSH Agent 等可以绕过文件边界的 IPC 入口。
fn inherit_host_env(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    if lowered.starts_with("dbus_") || lowered.starts_with("pulse_") {
        return false;
    }
    !matches!(
        key,
        "DISPLAY"
            | "WAYLAND_DISPLAY"
            | "XAUTHORITY"
            | "XDG_RUNTIME_DIR"
            | "XDG_ACTIVATION_TOKEN"
            | "DESKTOP_STARTUP_ID"
            | "SESSION_MANAGER"
            | "NOTIFY_SOCKET"
            | "SSH_AUTH_SOCK"
            | "SSH_AGENT_PID"
            | "GPG_AGENT_INFO"
            | "LD_PRELOAD"
            | "LD_LIBRARY_PATH"
            | "LD_AUDIT"
            | "LD_DEBUG"
            | "MALLOC_PERTURB_"
            | "SUJIAN_BWRAP"
            | "OLDPWD"
            | "PWD"
            | "TMPDIR"
    )
}

fn push_writable_dir(
    command: &mut Command,
    path: &Path,
    mounted: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    if !path.is_absolute() || !mounted.insert(path.to_path_buf()) {
        return Ok(());
    }
    let Some(directory) = ensure_dir(path) else {
        return Err(format!(
            "无法创建 Agent 配置目录，可能导致 CLI 读到空配置：{}",
            path.display()
        ));
    };
    command.arg("--bind").arg(&directory).arg(path);
    Ok(())
}

fn push_existing_file(
    command: &mut Command,
    path: &Path,
    writable: bool,
    mounted: &mut HashSet<PathBuf>,
) {
    if !path.is_absolute() || !path.is_file() || !mounted.insert(path.to_path_buf()) {
        return;
    }
    command
        .arg(if writable { "--bind" } else { "--ro-bind" })
        .arg(path)
        .arg(path);
}

fn ensure_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    std::fs::create_dir_all(path).ok()?;
    Some(path.to_path_buf())
}

fn push_existing_ro(command: &mut Command, path: &Path) {
    if path.exists() {
        command.arg("--ro-bind").arg(path).arg(path);
    }
}

impl WorkspaceAccess {
    pub fn for_context(ctx: &AgentLaunchContext) -> Result<Self, String> {
        let bwrap = resolve_bwrap()?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .ok_or_else(|| "无法确定当前用户 HOME，不能建立 Agent 沙箱".to_string())?;
        let mut extra_ro_paths = Vec::new();
        if let Some(appimage) = std::env::var_os("APPIMAGE") {
            let path = PathBuf::from(appimage);
            if path.exists() {
                extra_ro_paths.push(path);
            }
        }
        for server in &ctx.mcp_servers {
            let path = PathBuf::from(&server.command);
            if path.exists() {
                extra_ro_paths.push(path);
            }
        }
        Ok(Self {
            bwrap,
            home,
            agent_id: ctx.agent_id.clone(),
            projects_root: ctx.projects_root.clone(),
            working_dir: ctx.working_dir.clone(),
            mcp_socket_dir: ctx.mcp_socket_dir.clone(),
            extra_ro_paths,
        })
    }

    /// 生成 bwrap 命令；调用方追加的 Agent 参数会落在 `-- program` 之后。
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command, String> {
        let mut command = Command::new(&self.bwrap);
        command.env_clear();
        // 继承用户启动 CLI 时的真实配置环境：自定义 XDG 路径、CLI 配置目录
        // 和认证变量必须与宿主一致，否则目录探测与真实会话会看到不同的
        // provider。这里不枚举任何厂商 API Key，只排除会话 IPC 与动态链接
        // 注入变量。
        for (key, value) in std::env::vars_os() {
            let key_text = key.to_string_lossy();
            if inherit_host_env(&key_text) {
                command.env(&key, &value);
            }
        }
        let config_home = env_absolute_path("XDG_CONFIG_HOME")
            .unwrap_or_else(|| self.home.join(".config"));
        let data_home =
            env_absolute_path("XDG_DATA_HOME").unwrap_or_else(|| self.home.join(".local/share"));
        let state_home =
            env_absolute_path("XDG_STATE_HOME").unwrap_or_else(|| self.home.join(".local/state"));
        let cache_home =
            env_absolute_path("XDG_CACHE_HOME").unwrap_or_else(|| self.home.join(".cache"));
        command
            .env("HOME", &self.home)
            .env("PWD", &self.working_dir)
            .env("TMPDIR", "/tmp")
            .env("XDG_CACHE_HOME", &cache_home)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("XDG_DATA_HOME", &data_home)
            .env("XDG_STATE_HOME", &state_home);

        // 运行时与系统配置：只读。
        for dir in ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/opt"] {
            let path = Path::new(dir);
            if path.exists() {
                command.arg("--ro-bind").arg(path).arg(path);
            }
        }

        command
            .arg("--dev")
            .arg("/dev")
            .arg("--proc")
            .arg("/proc")
            .arg("--tmpfs")
            .arg("/tmp")
            .arg("--tmpfs")
            .arg("/dev/shm");
        if Path::new("/dev/fuse").exists() {
            command
                .arg("--dev-bind")
                .arg("/dev/fuse")
                .arg("/dev/fuse");
        }
        // 仅 DNS 解析所需的 systemd-resolved 目录；不暴露 /run/user。
        push_existing_ro(&mut command, Path::new("/run/systemd/resolve"));

        let mut mounted = HashSet::new();
        command.arg("--dir").arg(&self.home);
        for dir in agent_state_dirs(
            &self.agent_id,
            &self.home,
            &config_home,
            &data_home,
            &state_home,
            &cache_home,
        ) {
            push_writable_dir(&mut command, &dir, &mut mounted)?;
        }
        // 用户显式设置的 CLI 配置目录/文件可能位于 HOME 之外，按实际路径
        // 精确挂载；不读取也不记录认证文件内容。OpenCode 的
        // ~/.local/share/opencode/auth.json 已随数据目录整体可见，避免
        // 沙箱内看不到已登录的付费供应商。
        for key in ["CODEX_HOME", "OPENCODE_CONFIG_DIR"] {
            if let Some(path) = env_absolute_path(key) {
                push_writable_dir(&mut command, &path, &mut mounted)?;
            }
        }
        for key in ["OPENCODE_CONFIG", "OPENCODE_TUI_CONFIG"] {
            if let Some(path) = env_absolute_path(key) {
                push_existing_file(&mut command, &path, false, &mut mounted);
            }
        }
        if let Some(path) = env_absolute_path("OPENCODE_MODELS_PATH") {
            push_existing_file(&mut command, &path, true, &mut mounted);
        }
        for key in ["SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "GIT_SSL_CAINFO"] {
            if let Some(path) = env_absolute_path(key) {
                push_existing_file(&mut command, &path, false, &mut mounted);
            }
        }
        // 只读保留已安装 CLI 的运行时目录。
        for dir in [
            ".local/bin",
            ".local/share/pnpm",
            ".local/share/mise",
            ".nvm",
            ".bun",
            ".volta",
            ".cargo/bin",
            ".opencode",
        ] {
            push_existing_ro(&mut command, &self.home.join(dir));
        }
        for path in [
            ".gitconfig",
            ".config/git",
            ".config/npm",
        ] {
            push_existing_ro(&mut command, &self.home.join(path));
        }

        command
            .arg("--ro-bind")
            .arg(&self.projects_root)
            .arg(&self.projects_root);
        if let Some(socket_dir) = self.mcp_socket_dir.as_ref() {
            command.arg("--ro-bind").arg(socket_dir).arg(socket_dir);
        }
        for path in &self.extra_ro_paths {
            if !path.exists() {
                continue;
            }
            let canonical = canonical_for_sandbox(path, "Agent 依赖")?;
            if forbidden_host_path(&canonical) || forbidden_host_path(path) {
                return Err(format!(
                    "Agent 依赖位于宿主用户会话目录，不能暴露给沙箱：{}",
                    path.display()
                ));
            }
            // 已随 /usr 等运行时目录可见的路径不重复挂载。
            if trusted_runtime_path(path) && trusted_runtime_path(&canonical) {
                continue;
            }
            command.arg("--ro-bind").arg(&canonical).arg(path);
        }

        let program_path = PathBuf::from(program);
        let program_canonical = canonical_for_sandbox(&program_path, "Agent 可执行文件")?;
        if forbidden_host_path(&program_path) || forbidden_host_path(&program_canonical) {
            return Err(format!(
                "Agent 可执行文件位于宿主用户会话目录，不能在沙箱中运行：{}",
                program_path.display()
            ));
        }
        if !trusted_runtime_path(&program_canonical) {
            // 自定义安装只暴露精确的可执行文件及最小运行时目录，绝不挂载
            // 任意父目录，避免从 /run/user 等路径重新引入宿主 IPC。
            command
                .arg("--ro-bind")
                .arg(&program_canonical)
                .arg(&program_canonical);
            if let Some(parent) = program_canonical.parent() {
                for name in ["lib", "lib64", "share"] {
                    let directory = parent.join(name);
                    if directory.is_dir() {
                        command.arg("--ro-bind").arg(&directory).arg(&directory);
                    }
                }
            }
            if let Some(interpreter) = script_interpreter(&program_canonical) {
                let canonical = canonical_for_sandbox(&interpreter, "Agent 解释器")?;
                if forbidden_host_path(&interpreter) || forbidden_host_path(&canonical) {
                    return Err(format!(
                        "Agent 解释器位于宿主用户会话目录，不能暴露给沙箱：{}",
                        interpreter.display()
                    ));
                }
                if !trusted_runtime_path(&canonical) {
                    command.arg("--ro-bind").arg(&canonical).arg(&interpreter);
                }
            }
        }

        command
            .arg("--chdir")
            .arg(&self.working_dir)
            .arg("--unshare-ipc")
            .arg("--unshare-pid")
            .arg("--unshare-uts")
            .arg("--unshare-cgroup-try")
            .arg("--cap-drop")
            .arg("ALL")
            .arg("--die-with-parent")
            .arg("--new-session")
            .arg("--")
            .arg(&program_canonical)
            .args(args);
        Ok(command)
    }
}

fn trusted_runtime_path(path: &Path) -> bool {
    ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/opt"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn forbidden_host_path(path: &Path) -> bool {
    // 这些目录包含宿主用户会话、进程、设备和运行时 IPC。即使只读挂载，
    // 也允许连接 Unix Socket，因此不能把它们重新带回沙箱。
    ["/run", "/var/run", "/proc", "/sys", "/dev"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn canonical_for_sandbox(path: &Path, label: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|error| format!("无法解析{label}路径 {}：{error}", path.display()))
}

fn script_interpreter(path: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(path).ok()?;
    let first = bytes.split(|byte| *byte == b'\n').next()?;
    let line = first.strip_prefix(b"#!")?;
    let line = String::from_utf8_lossy(line);
    let interpreter = line.split_whitespace().next()?;
    Some(PathBuf::from(interpreter))
}
