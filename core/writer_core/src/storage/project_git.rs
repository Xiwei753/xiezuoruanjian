//! # 作品 Git 仓库（Core 层基础设施）
//!
//! 每个作品目录自身就是 Git 仓库（Issue #600）。Git 是作品存储基础，
//! 与网络同步能力（`git-https` feature）解耦：本地仓库始终存在，
//! `git-https` 只控制网络 HTTPS 能力。

use std::path::Path;

use crate::storage::git_repo_layout::GitRepoLayout;

/// 确保 `project_root` 是一个已初始化的 Git 仓库（标准 Git 布局）。
///
/// - 已存在 `.git/` 时直接返回（幂等）。
/// - 不存在时执行 `git init`，用于新建作品以及把旧作品永久迁移成 Git 仓库。
///
/// 本地仓库能力是 Core 基础依赖，不依赖 `git-https` feature。
pub fn ensure_project_repo(project_root: &Path) -> crate::Result<()> {
    let layout = GitRepoLayout::new(project_root.to_path_buf());
    crate::storage::git_repo_layout::ensure_project_repo_with_layout(&layout)
}

/// 确保仓库使用指定的 `layout` 初始化（支持外部 git_dir）。
///
/// 当 `git_dir` 与 `worktree_root/.git` 不同时，使用 libgit2 的独立 workdir
/// 能力初始化/打开仓库。
pub fn ensure_project_repo_with_layout(layout: &GitRepoLayout) -> crate::Result<()> {
    crate::storage::git_repo_layout::ensure_project_repo_with_layout(layout)
}
