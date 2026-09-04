//! #644 评论 5489750244 问题1：Git 仓库布局模型。
//!
//! 将 worktree（用户可见文件）与 git_dir（可写 metadata）分离。
//! Android 共享存储不适合放可写 Git metadata，因为 sidecar 文件与真正的 `.lock`
//! 不是原子事实，无法可靠证明 ownership。本模块允许 git_dir 放在应用私有目录。
//!
//! #645 评论 5504296097 第2点：本模块只保留布局模型和迁移工具。
//! 初始化入口统一走 `workspace_git::ensure_workspace_repo()`。

mod migration;

#[cfg(test)]
pub(crate) use migration::{
    canonicalize_or_lossy, journal_path, migrate_copy_dir_recursive, write_migration_journal,
    LayoutMigrationJournal, MigrationPhase, LAYOUT_MIGRATIONS_DIR, LAYOUT_MIGRATION_JOURNAL_NAME,
};

use std::path::{Path, PathBuf};

/// 评论 5489750244 问题1：明确的 Git 布局模型。
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

    pub fn with_external_git_dir(worktree_root: PathBuf, git_dir: PathBuf) -> Self {
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
    let default_git_dir = layout.worktree_root.join(".git");
    if layout.git_dir == default_git_dir {
        git2::Repository::open(&layout.worktree_root)
    } else {
        let repo = git2::Repository::open(&layout.git_dir)?;
        repo.set_workdir(&layout.worktree_root, false)?;
        Ok(repo)
    }
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

/// 确保 Git 仓库存在（init if missing）并处理布局迁移。
///
/// #645 评论 5504296097 第2点：公开入口统一走 `workspace_git::ensure_workspace_repo()`，
/// 本函数是内部实现，由 `workspace_git::repo` 委托调用。
pub(crate) fn ensure_repo_layout_initialized(layout: &GitRepoLayout) -> crate::Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    migration::resume_layout_migration(layout)?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
            if default_git_dir.exists() && is_external {
                migration::complete_migration_with_journal(
                    &layout.git_dir,
                    &default_git_dir,
                    &layout.worktree_root,
                )?;
            }
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

    if default_git_dir.exists() {
        match try_open_repo(&default_git_dir)? {
            RepoOpenResult::Valid => {
                if is_external {
                    migration::migrate_embedded_git(
                        &default_git_dir,
                        &layout.git_dir,
                        &layout.worktree_root,
                    )?;
                    return Ok(());
                }
                return Ok(());
            }
            RepoOpenResult::Corrupt(e) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "ensure_repo_layout_initialized: embedded .git exists but is corrupt: {}",
                    e,
                ))));
            }
            RepoOpenResult::Missing => {}
        }
    }

    if let Some(parent) = layout.git_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if is_external {
        let mut opts = git2::RepositoryInitOptions::new();
        opts.no_dotgit_dir(true);
        git2::Repository::init_opts(&layout.git_dir, &opts).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_repo_layout_initialized: init_opts({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        let repo = git2::Repository::open(&layout.git_dir).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_repo_layout_initialized: open after init({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        repo.set_workdir(&layout.worktree_root, false)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_repo_layout_initialized: set_workdir({}): {}",
                    layout.worktree_root.display(),
                    e,
                )))
            })?;
        let mut config = repo.config().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_repo_layout_initialized: config({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        config
            .set_str("core.worktree", &layout.worktree_root.to_string_lossy())
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_repo_layout_initialized: set_str core.worktree({}): {}",
                    layout.git_dir.display(),
                    e,
                )))
            })?;
    } else {
        git2::Repository::init(&layout.worktree_root).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_repo_layout_initialized: init({}): {}",
                layout.worktree_root.display(),
                e,
            )))
        })?;
    }
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

    migration::resume_layout_migration(layout)?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
            if default_git_dir.exists() && is_external {
                migration::complete_migration_with_journal(
                    &layout.git_dir,
                    &default_git_dir,
                    &layout.worktree_root,
                )?;
            }
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

    if default_git_dir.exists() {
        match try_open_repo(&default_git_dir)? {
            RepoOpenResult::Valid => {
                if is_external {
                    migration::migrate_embedded_git(
                        &default_git_dir,
                        &layout.git_dir,
                        &layout.worktree_root,
                    )?;
                    return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
                }
                return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
            }
            RepoOpenResult::Corrupt(e) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "resolve_existing_repo_layout: embedded .git exists but is corrupt: {}",
                    e,
                ))));
            }
            RepoOpenResult::Missing => {}
        }
    }

    Ok(ExistingRepoLayoutState::NotGitRepo)
}

pub fn open_repo(layout: &GitRepoLayout) -> std::result::Result<git2::Repository, crate::Error> {
    open_repo_with_layout(layout)
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("open_repo: {}", e))))
}

#[cfg(test)]
mod tests;
