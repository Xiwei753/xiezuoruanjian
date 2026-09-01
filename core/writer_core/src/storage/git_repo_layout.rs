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
/// - 如果 `worktree_root` 有 `.git`，迁移到 `git_dir` 位置（跨文件系统安全）。
/// - 否则在 `git_dir` 位置 init 一个新仓库，workdir 指向 `worktree_root`。
///
/// ## 新仓库初始化
///
/// `Repository::init(path)` 把 `path` 当工作目录并在其下创建 `.git/`。
/// 当 `git_dir` 与 `worktree_root/.git` 不同时（Android 外部 metadata），
/// 必须用 `RepositoryInitOptions::no_dotgit_dir(true)` + `workdir_path()` 让
/// libgit2 把 `git_dir` 当成 `.git` 等价位置，workdir 指向 `worktree_root`。
/// 参考：https://docs.rs/git2/latest/git2/struct.RepositoryInitOptions.html#method.workdir_path
///
/// ## 跨文件系统迁移
///
/// Android 共享存储 → 应用私有 `filesDir` 属于不同 mount/filesystem，
/// `std::fs::rename` 跨文件系统会失败（Rust 文档明确说明）。
/// 迁移必须在目标文件系统上建 tmp 目录、递归复制、fsync、原子 rename。
pub fn ensure_project_repo_with_layout(
    layout: &GitRepoLayout,
) -> crate::Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    // 1. git_dir 已有仓库 → 幂等返回。
    if git2::Repository::open(&layout.git_dir).is_ok() {
        return Ok(());
    }

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    // 2. worktree_root 有内嵌 .git → 迁移到外部 git_dir。
    if default_git_dir.exists() && is_external {
        migrate_embedded_git(&default_git_dir, &layout.git_dir, &layout.worktree_root)?;
        return Ok(());
    }

    // 3. 全新仓库：在 git_dir 位置 init，workdir 指向 worktree_root。
    if let Some(parent) = layout.git_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if is_external {
        // git_dir 与 worktree_root 分离，使用 RepositoryInitOptions。
        // no_dotgit_dir(true)：不在 git_dir 内创建 .git 子目录，
        // git_dir 本身就是 .git 等价位置。
        // workdir_path：设置仓库的工作目录为 worktree_root。
        let mut opts = git2::RepositoryInitOptions::new();
        opts.no_dotgit_dir(true);
        opts.workdir_path(&layout.worktree_root);
        git2::Repository::init_opts(&layout.git_dir, &opts)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: init_opts({}): {}",
                    layout.git_dir.display(),
                    e,
                )))
            })?;
    } else {
        // 标准布局：git_dir == worktree_root.join(".git")，
        // Repository::init(worktree_root) 会自动在 worktree_root 下创建 .git。
        // 但这里 layout.git_dir 已经是 .git 路径，需要 init worktree_root。
        git2::Repository::init(&layout.worktree_root)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: init({}): {}",
                    layout.worktree_root.display(),
                    e,
                )))
            })?;
    }
    Ok(())
}

/// #644 评论 5490799656 问题2：RAII 守卫，保证临时目录在 drop 时删除。
struct MigrateTmpDirGuard(Option<PathBuf>);

impl MigrateTmpDirGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn path(&self) -> &Path {
        self.0.as_ref().expect("MigrateTmpDirGuard already disarmed")
    }

    fn disarm(&mut self) -> PathBuf {
        self.0.take().expect("MigrateTmpDirGuard already disarmed")
    }
}

impl Drop for MigrateTmpDirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// #644 评论 5490799656 问题2：递归复制目录（durable copy）。
///
/// 每个文件 copy 后 fsync + 父目录 fsync。每层目录递归返回后 sync 当前 dst 目录。
fn migrate_copy_dir_recursive(src: &Path, dst: &Path) -> crate::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            migrate_copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            crate::storage::durable_copy_file(&src_path, &dst_path)?;
        }
    }
    crate::storage::sync_dir(dst)?;
    Ok(())
}

/// #644 评论 5490799656 问题2：跨文件系统安全的 .git 迁移。
///
/// Android 共享存储 → 应用私有 `filesDir` 属于不同 mount/filesystem，
/// `std::fs::rename` 跨文件系统会返回 `InvalidCrossDeviceLink`。
///
/// 正确流程：
/// 1. 在目标文件系统（`git_dir` 的父目录）建 tmp 目录；
/// 2. 递归复制旧 `.git` 到 tmp（每个文件 fsync + 目录 fsync）；
/// 3. 打开 tmp repo 确认 HEAD/refs/index 可读；
/// 4. 在同一文件系统内原子 rename tmp → final git_dir；
/// 5. 设置正确 workdir；
/// 6. 清理旧 `.git`。
fn migrate_embedded_git(
    default_git_dir: &Path,
    target_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    // 1. 在目标文件系统上建 tmp 目录（与 target_git_dir 同一文件系统）。
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let target_parent = target_git_dir.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let tmp_git = target_parent.join(format!(".git.sujian-migrate-{}", tmp_id));
    let mut guard = MigrateTmpDirGuard::new(tmp_git);

    // 2. 递归复制旧 .git 到 tmp（含 fsync）。
    migrate_copy_dir_recursive(default_git_dir, guard.path())?;

    // 3. 打开 tmp repo 确认可读。
    {
        let tmp_repo = git2::Repository::open(guard.path()).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "migrate_embedded_git: open tmp repo: {e}"
            )))
        })?;
        // 验证 HEAD 可读（如果有的话）。
        let _ = tmp_repo.head();
        let _ = tmp_repo.find_reference("HEAD");
    }

    // 4. 在同一文件系统内原子 rename tmp → final git_dir。
    if let Some(parent) = target_git_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let guard_path = guard.disarm();
    std::fs::rename(&guard_path, target_git_dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&guard_path);
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: rename {} -> {}: {}",
            guard_path.display(),
            target_git_dir.display(),
            e,
        )))
    })?;
    crate::storage::sync_dir(target_git_dir)?;

    // 5. 设置 workdir。
    let repo = git2::Repository::open(target_git_dir).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: open migrated repo: {e}"
        )))
    })?;
    repo.set_workdir(worktree_root, false).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: set_workdir: {e}"
        )))
    })?;

    // 6. 清理旧 .git。
    std::fs::remove_dir_all(default_git_dir).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: remove old .git: {e}"
        )))
    })?;
    crate::storage::sync_dir(worktree_root)?;

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
