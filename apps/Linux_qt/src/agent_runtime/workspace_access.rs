//! Agent 进程的作品文件系统边界。
//!
//! 侧载 Agent 自带 Shell/文件编辑工具，MCP 的只读正文工具无法限制它直接改
//! 作品目录。这里用 bubblewrap 把真实作品根和 Sujian 应用数据根以只读方式
//! 覆盖挂载，Agent 及其所有子进程都继承同一挂载命名空间；需要写入的元数据
//! 由沙箱外的 `sujian_mcp` broker 通过 Core 事务完成。
//!
//! 用户自己的 Agent 配置/登录状态仍可写（$HOME 以读写方式挂载），避免破坏
//! Codex/Claude/Gemini/OpenCode 的既有认证环境。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::events::AgentLaunchContext;

#[derive(Debug, Clone)]
pub struct WorkspaceAccess {
    bwrap: PathBuf,
    app_data_root: PathBuf,
    projects_root: PathBuf,
    working_dir: PathBuf,
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

fn resolve_bwrap() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("SUJIAN_BWRAP") {
        let path = PathBuf::from(value);
        if executable(&path) {
            return Ok(path);
        }
        return Err(format!(
            "SUJIAN_BWRAP 指向的 bubblewrap 不可执行：{}",
            path.display()
        ));
    }
    for candidate in ["/usr/bin/bwrap", "/usr/local/bin/bwrap"] {
        let path = PathBuf::from(candidate);
        if executable(&path) {
            return Ok(path);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("bwrap");
            if executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err("未找到 bubblewrap（bwrap）；侧载 Agent 必须运行在只读作品沙箱中，请安装 bubblewrap 后重试".to_string())
}

impl WorkspaceAccess {
    pub fn for_context(ctx: &AgentLaunchContext) -> Result<Self, String> {
        Ok(Self {
            bwrap: resolve_bwrap()?,
            app_data_root: ctx.app_data_root.clone(),
            projects_root: ctx.projects_root.clone(),
            working_dir: ctx.working_dir.clone(),
        })
    }

    /// 构造以 `program args` 为最终命令的沙箱进程。
    ///
    /// 调用方可以继续追加 Agent 自己的参数；它们会落在 `--` 之后、属于
    /// 被沙箱包裹的真实程序，而不会变成 bwrap 参数。
    pub fn command(&self, program: &str, args: &[String]) -> Command {
        let mut command = Command::new(&self.bwrap);
        command
            .arg("--ro-bind")
            .arg("/")
            .arg("/")
            .arg("--dev")
            .arg("/dev")
            // `--dev` 只创建基础设备节点；AppImage 内的 MCP stdio bridge 仍要
            // 通过 FUSE 重新挂载自身，因此按需把宿主 /dev/fuse 传进沙箱。
            .arg("--proc")
            .arg("/proc");
        if Path::new("/dev/fuse").exists() {
            command
                .arg("--dev-bind")
                .arg("/dev/fuse")
                .arg("/dev/fuse");
        }
        command.arg("--bind").arg("/tmp").arg("/tmp");
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            if home.is_dir() {
                command.arg("--bind").arg(&home).arg(&home);
            }
        }
        // 覆盖挂载顺序很重要：先让 HOME 可写，再把作品和应用数据重新压回只读。
        command
            .arg("--ro-bind")
            .arg(&self.app_data_root)
            .arg(&self.app_data_root)
            .arg("--ro-bind")
            .arg(&self.projects_root)
            .arg(&self.projects_root);
        if self.working_dir != self.projects_root && self.working_dir.is_dir() {
            command
                .arg("--ro-bind")
                .arg(&self.working_dir)
                .arg(&self.working_dir);
        }
        command
            .arg("--chdir")
            .arg(&self.working_dir)
            .arg("--die-with-parent")
            .arg("--new-session")
            .arg("--")
            .arg(program)
            .args(args);
        command
    }
}
