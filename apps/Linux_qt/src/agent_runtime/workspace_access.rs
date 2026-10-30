//! Side-loaded Agent 的 Linux 文件系统边界。
//!
//! Agent 只能看到运行所需的系统目录、当前作品（只读）、自身认证/状态目录
//! （可写）和 Sujian MCP broker 的 Unix Socket。宿主 `/run/user`、D-Bus、
//! systemd user manager、X11/Wayland 等会话 IPC 不进入沙箱，进程环境也按
//! 白名单重建，避免通过 `systemd-run --user` 等方式绕过 Core 直接改作品。

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

fn agent_state_dirs(agent_id: &str, home: &Path) -> Vec<PathBuf> {
    let mut names: Vec<&str> = match agent_id {
        "claude" => vec!["claude", "claude-code"],
        "codex" => vec!["codex"],
        "gemini" => vec!["gemini"],
        "opencode" => vec!["opencode"],
        other if !other.is_empty() => vec![other],
        _ => Vec::new(),
    };
    names.sort_unstable();
    names.dedup();

    let mut dirs = Vec::new();
    for name in names {
        dirs.push(home.join(format!(".{name}")));
        dirs.push(home.join(".config").join(name));
        dirs.push(home.join(".local/share").join(name));
        dirs.push(home.join(".local/state").join(name));
        dirs.push(home.join(".cache").join(name));
    }
    dirs
}

fn ensure_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    std::fs::create_dir_all(path).ok()?;
    Some(path.to_path_buf())
}

fn ensure_json_file(path: &Path) -> Option<PathBuf> {
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_file() && metadata.len() > 0 {
            return Some(path.to_path_buf());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    // Claude 首次启动需要可写的 ~/.claude.json；空文件不是合法 JSON。
    std::fs::write(path, b"{}").ok()?;
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
    pub fn command(&self, program: &str, args: &[String]) -> Command {
        let mut command = Command::new(&self.bwrap);
        command.env_clear();
        // 只保留定位运行时、语言、证书和代理所需的变量。DBUS/XDG_RUNTIME_DIR
        // 等会话通信变量一律不传入。
        for key in [
            "PATH",
            "LANG",
            "LC_ALL",
            "TERM",
            "SHELL",
            "USER",
            "LOGNAME",
            "http_proxy",
            "https_proxy",
            "no_proxy",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "NODE_EXTRA_CA_CERTS",
            "GIT_SSL_CAINFO",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("HOME", &self.home)
            .env("TMPDIR", "/tmp")
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"));

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

        command.arg("--dir").arg(&self.home);
        for dir in agent_state_dirs(&self.agent_id, &self.home) {
            if let Some(dir) = ensure_dir(&dir) {
                command.arg("--bind").arg(&dir).arg(&dir);
            }
        }
        if self.agent_id == "claude" {
            let config = self.home.join(".claude.json");
            if let Some(config) = ensure_json_file(&config) {
                command.arg("--bind").arg(&config).arg(&config);
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
            command.arg("--ro-bind").arg(path).arg(path);
        }
        if program_path_is_outside_runtime(program) {
            let path = PathBuf::from(program);
            if let Some(parent) = path.parent() {
                if parent.exists() {
                    command.arg("--ro-bind").arg(parent).arg(parent);
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
            .arg(program)
            .args(args);
        command
    }
}

fn program_path_is_outside_runtime(program: &str) -> bool {
    let path = Path::new(program);
    !path.starts_with("/usr")
        && !path.starts_with("/bin")
        && !path.starts_with("/sbin")
        && !path.starts_with("/lib")
        && !path.starts_with("/opt")
}
