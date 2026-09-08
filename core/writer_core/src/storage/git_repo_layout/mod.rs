//! Git 仓库布局模型。
//!
//! git_dir 固定在 `worktree_root/.git`（标准 Git 布局）。
//! 初始化入口统一走 `workspace_git::ensure_workspace_repo()`。

use std::path::{Path, PathBuf};

/// 明确的 Git 布局模型。
#[derive(Debug, Clone)]
pub struct GitRepoLayout {
    pub worktree_root: PathBuf,
    pub git_dir: PathBuf,
}

impl GitRepoLayout {
    pub fn new(worktree_root: PathBuf) -> Self {
        let git_dir = worktree_root.join(".git");
        Self {
            worktree_root,
            git_dir,
        }
    }
}

pub fn legacy_default_git_dir(live_root: &Path) -> PathBuf {
    live_root.join(".git")
}

pub fn open_repo_with_layout(
    layout: &GitRepoLayout,
) -> std::result::Result<git2::Repository, git2::Error> {
    git2::Repository::open(&layout.worktree_root)
}

pub(crate) enum RepoOpenResult {
    Missing,
    Valid,
    Corrupt(git2::Error),
}

pub(crate) fn try_open_repo(path: &Path) -> crate::Result<RepoOpenResult> {
    if !path.exists() {
        return Ok(RepoOpenResult::Missing);
    }
    match git2::Repository::open(path) {
        Ok(_) => Ok(RepoOpenResult::Valid),
        Err(e) => Ok(RepoOpenResult::Corrupt(e)),
    }
}

/// 确保 Git 仓库存在（init if missing）。
///
/// #645 评论 5504296097 第2点：公开入口统一走 `workspace_git::ensure_workspace_repo()`，
/// 本函数是内部实现，由 `workspace_git::repo` 委托调用。
pub(crate) fn ensure_repo_layout_initialized(layout: &GitRepoLayout) -> crate::Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
            return Ok(());
        }
        RepoOpenResult::Corrupt(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "ensure_repo_layout_initialized: git_dir exists but is corrupt: {}: {}",
                layout.git_dir.display(),
                e,
            ))));
        }
        RepoOpenResult::Missing => {}
    }

    git2::Repository::init(&layout.worktree_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "ensure_repo_layout_initialized: init({}): {}",
            layout.worktree_root.display(),
            e,
        )))
    })?;
    Ok(())
}

#[derive(Debug, Clone)]
pub enum ExistingRepoLayoutState {
    NotGitRepo,
    Ready(GitRepoLayout),
}

pub fn resolve_existing_repo_layout(
    layout: &GitRepoLayout,
) -> crate::Result<ExistingRepoLayoutState> {
    crate::storage::git_runtime::ensure_initialized()?;

    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
            return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
        }
        RepoOpenResult::Corrupt(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "resolve_existing_repo_layout: git_dir exists but is corrupt: {}: {}",
                layout.git_dir.display(),
                e,
            ))));
        }
        RepoOpenResult::Missing => {}
    }

    Ok(ExistingRepoLayoutState::NotGitRepo)
}

pub fn open_repo(layout: &GitRepoLayout) -> std::result::Result<git2::Repository, crate::Error> {
    open_repo_with_layout(layout)
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("open_repo: {}", e))))
}
