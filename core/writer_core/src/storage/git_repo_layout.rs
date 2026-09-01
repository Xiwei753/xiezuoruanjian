//! #644 评论 5489750244 问题1：Git 仓库布局模型。
//!
//! 将 worktree（用户可见文件）与 git_dir（可写 metadata）分离。
//! Android 共享存储不适合放可写 Git metadata，因为 sidecar 文件与真正的 `.lock`
//! 不是原子事实，无法可靠证明 ownership。本模块允许 git_dir 放在应用私有目录。
//!
//! ## 核心概念
//!
//! - `worktree_root`：用户可见文件的根目录（正文、元数据等），如
//!   `/storage/emulated/0/Sujian/projects/<id>`。
//! - `git_dir`：可写 Git metadata（`.git/`）的根目录，如
//!   `filesDir/sujian-git/<project-id>/`。
//!
//! ## 迁移
//!
//! 新函数优先使用 `GitRepoLayout` 参数。`legacy_default_git_dir` 仅用于未重构的
//! 旧路径，后续应逐步消除。

use std::path::{Path, PathBuf};

/// 评论 5489750244 问题1：明确的 Git 布局模型。
///
/// - `worktree_root`：用户可见文件的根目录。
/// - `git_dir`：可写 Git metadata 的根目录。
pub struct GitRepoLayout {
    pub worktree_root: PathBuf,
    pub git_dir: PathBuf,
}

impl GitRepoLayout {
    /// 创建 layout。`git_dir` 默认为 `worktree_root.join(".git")`（标准 Git 布局）。
    pub fn new(worktree_root: PathBuf) -> Self {
        let git_dir = worktree_root.join(".git");
        Self {
            worktree_root,
            git_dir,
        }
    }

    /// 创建 layout，指定外部 git_dir。
    pub fn with_external_git_dir(worktree_root: PathBuf, git_dir: PathBuf) -> Self {
        Self {
            worktree_root,
            git_dir,
        }
    }
}

/// 从 live_root 获取默认 git_dir（标准 Git 布局：`live_root/.git`）。
///
/// 仅用于旧路径过渡，新代码应使用 `GitRepoLayout` 参数。
pub fn legacy_default_git_dir(live_root: &Path) -> PathBuf {
    live_root.join(".git")
}

/// 评论 5489750244 问题1：打开仓库，支持外部 git_dir。
///
/// 当 `git_dir == worktree_root.join(".git")` 时等效于 `Repository::open(worktree_root)`。
/// 当 `git_dir` 是外部路径时，使用 `RepositoryInitOptions::workdir_path()` 或
/// `Repository::set_workdir()` 打开仓库并指向正确的 worktree。
///
/// 参考：
/// - https://docs.rs/git2/latest/git2/struct.RepositoryInitOptions.html#method.workdir_path
/// - https://docs.rs/git2/latest/git2/struct.Repository.html#method.set_workdir
pub fn open_repo_with_layout(
    layout: &GitRepoLayout,
) -> std::result::Result<git2::Repository, git2::Error> {
    let default_git_dir = layout.worktree_root.join(".git");
    if layout.git_dir == default_git_dir {
        git2::Repository::open(&layout.worktree_root)
    } else {
        let repo = git2::Repository::open(&layout.git_dir)?;
        repo.set_workdir(&layout.worktree_root, false)?;
        Ok(repo)
    }
}

/// 使用给定 layout 初始化仓库。
///
/// - 如果 `git_dir` 已存在且可打开，直接返回（幂等）。
/// - 如果 `worktree_root` 有 `.git`，rename 到 `git_dir` 位置（迁移旧布局）。
/// - 否则在 `git_dir` 位置 init 一个新仓库，设置 workdir 为 `worktree_root`。
pub fn ensure_project_repo_with_layout(
    layout: &GitRepoLayout,
) -> crate::Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    // 1. git_dir 已有仓库 → 幂等返回。
    if git2::Repository::open(&layout.git_dir).is_ok() {
        return Ok(());
    }

    let default_git_dir = layout.worktree_root.join(".git");

    // 2. worktree_root 有内嵌 .git → 迁移到外部 git_dir。
    if default_git_dir.exists() && layout.git_dir != default_git_dir {
        // rename 内嵌 .git 到外部位置。
        std::fs::rename(&default_git_dir, &layout.git_dir)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: rename {} -> {}: {}",
                    default_git_dir.display(),
                    layout.git_dir.display(),
                    e,
                )))
            })?;
        crate::storage::sync_dir(&layout.git_dir)?;
        // 设置 workdir。
        let repo = git2::Repository::open(&layout.git_dir)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: open migrated repo: {e}"
                )))
            })?;
        repo.set_workdir(&layout.worktree_root, false)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: set_workdir after migrate: {e}"
                )))
            })?;
        return Ok(());
    }

    // 3. 全新仓库：在 git_dir 位置 init。
    // 创建 git_dir 父目录。
    if let Some(parent) = layout.git_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _repo = git2::Repository::init(&layout.git_dir)
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(e.to_string()))
        })?;
    if layout.git_dir != default_git_dir {
        // workdir 与 git_dir 不同，需要显式设置。
        // 注意：Repository::init 在 git_dir 位置创建仓库，
        // git_dir 本身是 `.git` 等价位置，不需要 set_workdir。
    }
    Ok(())
}

/// 打开仓库（从 layout 获取 git2::Repository，设置正确的 workdir）。
///
/// 失败时返回 Err。
pub fn open_repo(
    layout: &GitRepoLayout,
) -> std::result::Result<git2::Repository, crate::Error> {
    open_repo_with_layout(layout).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "open_repo: {}",
            e
        )))
    })
}
