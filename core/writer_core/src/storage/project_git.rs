//! # 作品 Git 仓库（Core 层基础设施）
//!
//! 每个作品目录自身就是 Git 仓库（Issue #600）。Git 是作品存储基础，
//! 与网络同步能力（`git-https` feature）解耦：本地仓库始终存在，
//! `git-https` 只控制网络 HTTPS 能力。

use std::path::Path;

/// 确保 `project_root` 是一个已初始化的 Git 仓库。
///
/// - 已存在 `.git/` 时直接返回（幂等）。
/// - 不存在时执行 `git init`，用于新建作品以及把旧作品永久迁移成 Git 仓库。
///
/// 本地仓库能力是 Core 基础依赖，不依赖 `git-https` feature。
pub fn ensure_project_repo(project_root: &Path) -> crate::Result<()> {
    if git2::Repository::open(project_root).is_ok() {
        return Ok(());
    }
    git2::Repository::init(project_root)
        .map(|_| ())
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))
}
