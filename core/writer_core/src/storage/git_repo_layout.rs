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

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 评论 5489750244 问题1：明确的 Git 布局模型。
///
/// - `worktree_root`：用户可见文件的根目录。
/// - `git_dir`：可写 Git metadata 的根目录。
#[derive(Debug, Clone)]
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

/// 评论 5489750244 问题1 + 评论 5491531984 问题2：打开仓库的唯一入口。
///
/// 当 `git_dir == worktree_root.join(".git")` 时等效于 `Repository::open(worktree_root)`。
/// 当 `git_dir` 是外部路径时，始终从 `git_dir` 打开仓库并调用
/// `Repository::set_workdir(worktree_root, false)` 指向正确的 worktree。
///
/// **不使用** `RepositoryInitOptions::workdir_path()`：该选项会在 worktree
/// 生成 `.git` gitlink 文件，与"共享目录不留 Git metadata"的目标冲突。
/// 初始化时通过 `set_workdir` + repo config `core.worktree` 绑定 worktree，
/// 不在 worktree 目录产生任何 Git 文件。
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
/// **不使用** `workdir_path()`——该选项会在 worktree 生成 `.git` gitlink 文件，
/// 与"共享目录不留 Git metadata"目标冲突（评论 5491531984 问题2）。
///
/// 正确做法：`no_dotgit_dir(true)` init 裸仓库在 `git_dir`，再
/// `Repository::open(git_dir)` + `set_workdir(worktree_root, false)` 绑定 worktree，
/// 最后持久化 `core.worktree` 到 repo config，确保后续 open 恢复正确。
///
/// 参考：
/// - https://docs.rs/git2/latest/git2/struct.RepositoryInitOptions.html#method.workdir_path
/// - https://docs.rs/git2/latest/git2/struct.Repository.html#method.set_workdir
///
/// ## 跨文件系统迁移
///
/// Android 共享存储 → 应用私有 `filesDir` 属于不同 mount/filesystem，
/// `std::fs::rename` 跨文件系统会失败（Rust 文档明确说明）。
/// 迁移必须在目标文件系统上建 tmp 目录、递归复制、fsync、原子 rename。
pub fn ensure_project_repo_with_layout(layout: &GitRepoLayout) -> crate::Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    // 1. git_dir 已有仓库 → 幂等返回。
    //    #644 评论 5491531984 问题3：如果 target 和 embedded .git 同时存在，
    //    不能直接 return Ok(())。先确认 private 是本次迁移出来的同一仓库，
    //    再完成删除 embedded .git。
    //    #644 评论 5493295108 问题3：改用 journal 状态机，恢复时只删 claimed_source，
    //    绝不能删除后来重新出现在 worktree/.git 的别人的仓库。
    if git2::Repository::open(&layout.git_dir).is_ok() {
        if default_git_dir.exists() && is_external {
            complete_migration_with_journal(
                &layout.git_dir,
                &default_git_dir,
                &layout.worktree_root,
            )?;
        }
        return Ok(());
    }

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
        // git_dir 与 worktree_root 分离。
        // #644 评论 5491531984 问题2：不用 workdir_path()——它会在 worktree
        // 生成 .git gitlink 文件。改用 no_dotgit_dir(true) init 裸仓库，
        // 再 set_workdir 绑定当前进程的 worktree，不生成任何共享目录文件。
        let mut opts = git2::RepositoryInitOptions::new();
        opts.no_dotgit_dir(true);
        git2::Repository::init_opts(&layout.git_dir, &opts).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_project_repo_with_layout: init_opts({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        // 打开刚 init 的仓库，设置 workdir 并持久化到 repo config。
        let repo = git2::Repository::open(&layout.git_dir).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_project_repo_with_layout: open after init({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        repo.set_workdir(&layout.worktree_root, false)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: set_workdir({}): {}",
                    layout.worktree_root.display(),
                    e,
                )))
            })?;
        // 持久化 core.worktree，确保后续 open_repo_with_layout 能恢复。
        let mut config = repo.config().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "ensure_project_repo_with_layout: config({}): {}",
                layout.git_dir.display(),
                e,
            )))
        })?;
        config
            .set_str("core.worktree", &layout.worktree_root.to_string_lossy())
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: set_str core.worktree({}): {}",
                    layout.git_dir.display(),
                    e,
                )))
            })?;
    } else {
        // 标准布局：git_dir == worktree_root.join(".git")，
        // Repository::init(worktree_root) 会自动在 worktree_root 下创建 .git。
        // 但这里 layout.git_dir 已经是 .git 路径，需要 init worktree_root。
        git2::Repository::init(&layout.worktree_root).map_err(|e| {
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

#[allow(clippy::expect_used)]
impl MigrateTmpDirGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn path(&self) -> &Path {
        self.0
            .as_ref()
            .expect("MigrateTmpDirGuard already disarmed")
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

/// #644 评论 5493295108 问题3：迁移 journal 文件名。
///
/// 写在 private git_dir 内部，作为"这个 private repo 是由某个 embedded .git
/// 迁移而来"的 durable ownership fact。迁移完全成功后才删除；中途 I/O 失败
/// 保留 journal，下次重启看到 journal 并尝试继续清理。
///
/// 关键改进：journal 记录 `claimed_source`（迁移时已取得所有权的 source 路径，
/// 通常形如 `worktree/.git.sujian-migrate-source-<owner>`）。恢复时只能删除
/// `claimed_source`，绝不能删除后来重新出现在 `worktree/.git` 的别人的仓库。
const LAYOUT_MIGRATION_JOURNAL_NAME: &str = ".sujian-layout-migration";

/// #644 评论 5493295108 问题3：迁移 journal 内容。
///
/// 写在 private git_dir 内的 `.sujian-layout-migration` 文件中，用
/// `crate::storage::atomic_write_bytes`（含 fsync 文件 + fsync 父目录）保证 durable。
///
/// journal 至少保存：owner, original_source, claimed_source, worktree_root,
/// target_git_dir, phase。恢复时只能删除 claimed_source，绝不能删除后来重新
/// 出现在 worktree/.git 的别人的仓库。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LayoutMigrationJournal {
    /// 本次迁移的唯一标识。
    migration_uuid: String,
    /// 迁移时 worktree_root 的规范路径。
    worktree_canonical: String,
    /// 迁移时 embedded .git 的原始路径（`worktree/.git`）。
    original_source: String,
    /// #644 评论 5493295108 问题3：迁移时已取得所有权的 source 路径。
    /// 通常形如 `worktree/.git.sujian-migrate-source-<owner>`。
    /// 恢复时只能删除这个路径，绝不能删除后来重新出现在 original_source 的仓库。
    claimed_source: String,
    /// 迁移的目标 private git_dir 路径。
    target_git_dir: String,
    /// 迁移当前阶段（"copied" / "finalized"）。
    phase: String,
}

/// #644 评论 5492740265 问题5：获取路径的规范形式。
///
/// `std::fs::canonicalize` 成功时返回规范路径；失败时回退到 `to_string_lossy`
/// （路径可能尚不存在或所在文件系统不支持 canonicalize）。
fn canonicalize_or_lossy(path: &Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// #644 评论 5493295108 问题3：写迁移 journal 到 git_dir 内。
///
/// 用 `crate::storage::atomic_write_bytes`（fsync 文件 + fsync 父目录）保证 durable。
fn write_migration_journal(git_dir: &Path, journal: &LayoutMigrationJournal) -> crate::Result<()> {
    let journal_path = git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME);
    let content = serde_json::to_vec(journal).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_migration_journal: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_bytes(&journal_path, &content)
}

/// #644 评论 5493295108 问题3：读迁移 journal。
///
/// 返回 `Ok(None)` 表示 journal 不存在；`Ok(Some)` 表示读取并解析成功；
/// `Err` 表示 IO 错误（非 NotFound）或 JSON 解析错误——调用方应返回 Err，
/// 不假装迁移完成。
fn read_migration_journal(git_dir: &Path) -> crate::Result<Option<LayoutMigrationJournal>> {
    let journal_path = git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME);
    match std::fs::read(&journal_path) {
        Ok(content) => {
            let journal: LayoutMigrationJournal =
                serde_json::from_slice(&content).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "read_migration_journal: parse {}: {e}",
                        journal_path.display(),
                    )))
                })?;
            Ok(Some(journal))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(crate::Error::Io(e)),
    }
}

/// #644 评论 5493295108 问题3：删除迁移 journal + fsync git_dir。
///
/// 迁移完全成功后才调用。fsync git_dir 持久化 journal 删除的目录项。
/// 失败时返回 Err（journal 拟留，下次重启继续）。
fn remove_migration_journal(git_dir: &Path) -> crate::Result<()> {
    let journal_path = git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME);
    std::fs::remove_file(&journal_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "remove_migration_journal: remove {}: {e}",
            journal_path.display(),
        )))
    })?;
    // fsync git_dir 持久化 journal 删除的目录项。
    crate::storage::sync_dir(git_dir)
}

/// #644 评论 5493295108 问题3：双仓库并存时的迁移恢复状态机（journal 版）。
///
/// 当 final git_dir 能 open + embedded .git 同时存在时调用：
/// 1. 读 final git_dir 中的 migration journal
/// 2. journal 不存在 → 无法证明 private repo 是由这个 embedded .git 迁移来的，
///    不删除 embedded .git，返回 Ok(())（保留两份，让用户/上层决定）
/// 3. journal 存在 → 验证 journal 中 worktree_canonical 与当前 worktree_root 匹配，
///    且 `claimed_source` 仍存在（这是迁移时已取得所有权的 source）。
/// 4. `claimed_source` 存在 → 删除 `claimed_source` + fsync worktree（错误返回 Err 保留 journal），
///    成功后删除 journal + fsync private git_dir（错误返回 Err 保留 journal）
/// 5. `claimed_source` 不存在但 `original_source`（worktree/.git）存在 →
///    这是别人后来新建的仓库，**不删除**，只清理 journal（terminal cleanup）。
/// 6. journal 读取失败（IO 错误）→ 返回 Err，不假装迁移完成
///
/// 关键改进：恢复时只能删除 `claimed_source`，绝不能删除后来重新出现在
/// `original_source`（worktree/.git）的别人的仓库。
fn complete_migration_with_journal(
    private_git_dir: &Path,
    embedded_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    let journal = match read_migration_journal(private_git_dir) {
        Ok(Some(j)) => j,
        Ok(None) => {
            // journal 不存在 → 无法证明 private repo 是由这个 embedded .git 迁移来的。
            // 不删除 embedded .git，保留两份，让用户/上层决定。
            log::warn!(
                "[git_repo_layout] dual repo coexist but no migration journal in private {}; \
                 keeping embedded .git at {}",
                private_git_dir.display(),
                embedded_git_dir.display(),
            );
            return Ok(());
        }
        Err(e) => {
            // journal 读取失败（IO 错误）→ 不假装迁移完成，返回 Err。
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "complete_migration_with_journal: read journal from {}: {}",
                private_git_dir.display(),
                e,
            ))));
        }
    };

    // 验证 journal 中的 worktree canonical path 与当前路径匹配。
    let current_worktree_canonical = canonicalize_or_lossy(worktree_root);
    if journal.worktree_canonical != current_worktree_canonical {
        // 不匹配 → 可能是不同 worktree 的迁移残留，不删除 embedded .git。
        log::warn!(
            "[git_repo_layout] migration journal worktree mismatch: \
             journal(worktree={}, original_source={}, claimed_source={}) vs \
             current(worktree={}); keeping embedded .git at {}",
            journal.worktree_canonical,
            journal.original_source,
            journal.claimed_source,
            current_worktree_canonical,
            embedded_git_dir.display(),
        );
        return Ok(());
    }

    // #644 评论 5493295108 问题3：恢复时只能删除 claimed_source，
    // 绝不能删除后来重新出现在 original_source（worktree/.git）的别人的仓库。
    let claimed_source_path = PathBuf::from(&journal.claimed_source);
    if claimed_source_path.exists() {
        // claimed_source 仍存在 → 删除它 + fsync worktree。
        // 错误返回 Err 保留 journal，下次重启继续。
        std::fs::remove_dir_all(&claimed_source_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "complete_migration_with_journal: remove claimed_source {}: {e}",
                claimed_source_path.display(),
            )))
        })?;
        if let Some(parent) = worktree_root.parent() {
            crate::storage::sync_dir(parent)?;
        }
        crate::storage::sync_dir(worktree_root)?;
    } else {
        // claimed_source 不存在 → 迁移时已删除或崩溃后已清理。
        // 此时 embedded_git_dir（original_source）存在的是别人后来新建的仓库，
        // **不删除**，只清理 journal（terminal cleanup）。
        log::info!(
            "[git_repo_layout] migration journal terminal cleanup: claimed_source {} \
             already removed; keeping current embedded .git at {} (may be a later-created repo)",
            journal.claimed_source,
            embedded_git_dir.display(),
        );
    }

    // 删除 journal + fsync private git_dir。
    // 错误返回 Err 保留 journal，下次重启继续。
    remove_migration_journal(private_git_dir)?;

    Ok(())
}

/// #644 评论 5493295108 问题3：跨文件系统安全的 .git 迁移（journal 状态机版）。
///
/// Android 共享存储 → 应用私有 `filesDir` 属于不同 mount/filesystem，
/// `std::fs::rename` 跨文件系统会返回 `InvalidCrossDeviceLink`。
///
/// #644 评论 5493295108 问题3 关键改进：先取得 source 的所有权，再复制。
/// 流程：
/// 1. 在 worktree 同文件系统原子 rename `worktree/.git` →
///    `worktree/.git.sujian-migrate-source-<owner>`（取得 source 所有权）；
/// 2. fsync worktree（持久化 rename）；
/// 3. 在目标文件系统（`git_dir` 的父目录）建 tmp 目录；
/// 4. 递归复制 owned source 到 tmp（每个文件 fsync + 目录 fsync）；
/// 5. 打开 tmp repo 确认 HEAD/refs/index 可读；
/// 6. 写 `.sujian-layout-migration` journal 到 tmp（含 claimed_source 等完整信息）；
/// 7. 在同一文件系统内原子 rename tmp → final git_dir（journal 跟着进入 final git_dir）；
/// 8. 设置正确 workdir；
/// 9. 删除 owned source（`claimed_source`）+ fsync worktree；
/// 10. 迁移完全成功后删除 journal + fsync private git_dir。
///
/// 恢复时（`complete_migration_with_journal`）只能删除 `claimed_source`，
/// 绝不能删除后来重新出现在 `worktree/.git` 的别人的仓库。
fn migrate_embedded_git(
    default_git_dir: &Path,
    target_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    let migration_uuid = uuid::Uuid::new_v4().to_string();

    // 1. 先取得 source 的所有权：原子 rename worktree/.git → owned source。
    //    同文件系统 rename 是原子的，要么成功要么失败，不会半完成。
    let owned_source_name = format!(".git.sujian-migrate-source-{}", migration_uuid);
    let owned_source_path = worktree_root.join(&owned_source_name);
    std::fs::rename(default_git_dir, &owned_source_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: rename source {} -> {}: {}",
            default_git_dir.display(),
            owned_source_path.display(),
            e,
        )))
    })?;
    // 2. fsync worktree（持久化 source rename 的目录项）。
    if let Some(parent) = worktree_root.parent() {
        crate::storage::sync_dir(parent)?;
    }
    crate::storage::sync_dir(worktree_root)?;

    // 3. 在目标文件系统上建 tmp 目录（与 target_git_dir 同一文件系统）。
    let target_parent = target_git_dir.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let tmp_git = target_parent.join(format!(".git.sujian-migrate-{}", migration_uuid));
    let mut guard = MigrateTmpDirGuard::new(tmp_git);

    // 4. 递归复制 owned source 到 tmp（含 fsync）。
    migrate_copy_dir_recursive(&owned_source_path, guard.path())?;

    // 5. 打开 tmp repo 确认可读。
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

    // 6. 在 rename 前写迁移 journal 到 tmp。
    //    journal 含完整信息：owner, original_source, claimed_source, worktree_root,
    //    target_git_dir, phase。用 atomic_write_bytes（fsync 文件 + fsync 父目录）保证 durable。
    //    rename 后 journal 跟着进入 final git_dir，作为 private repo 的 ownership fact。
    //    迁移完全成功后才删除 journal；中途 I/O 失败保留 journal，下次重启继续清理。
    let journal = LayoutMigrationJournal {
        migration_uuid: migration_uuid.clone(),
        worktree_canonical: canonicalize_or_lossy(worktree_root),
        original_source: default_git_dir.to_string_lossy().into_owned(),
        claimed_source: owned_source_path.to_string_lossy().into_owned(),
        target_git_dir: target_git_dir.to_string_lossy().into_owned(),
        phase: "copied".to_string(),
    };
    write_migration_journal(guard.path(), &journal)?;

    // 7. 在同一文件系统内原子 rename tmp → final git_dir。
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
    // rename 后先 fsync target_parent（目录项），再打开 final + 设置 workdir + 校验 refs/index。
    if let Some(parent) = target_git_dir.parent() {
        crate::storage::sync_dir(parent)?;
    }

    // 8. 设置 workdir 并验证仓库完整性。
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
    // 验证迁移后仓库的 refs/index 可读。
    let _ = repo.head();
    if let Ok(mut index) = repo.index() {
        let _ = index.read(true);
    }

    // 9. 删除 owned source（claimed_source）并 fsync worktree parent。
    //    这是恢复时唯一允许删除的 source 路径。
    std::fs::remove_dir_all(&owned_source_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: remove owned source {}: {e}",
            owned_source_path.display(),
        )))
    })?;
    // fsync worktree_root 的父目录（持久化 owned source 删除的目录项）。
    if let Some(parent) = worktree_root.parent() {
        crate::storage::sync_dir(parent)?;
    }
    crate::storage::sync_dir(worktree_root)?;

    // 10. 迁移完全成功后删除 journal + fsync private git_dir。
    //     如果这步失败，返回 Err（journal 拟留，下次重启看到 journal 并尝试继续清理）。
    remove_migration_journal(target_git_dir)?;

    Ok(())
}

/// #644 评论 5493295108 问题2：只处理"已有仓库位置"的 resolve/migrate 入口。
///
/// App target 用这个，不要"没有就 init"。语义定死：
/// - private git_dir 已有 repo → Ready
/// - private 没有 + worktree/.git 有 repo → 迁移后 Ready
/// - 两边都没有 → NotGitRepo
///
/// Project target 如果产品契约要求作品必有 Git，再在 resolve 后单独 init missing repo
/// （调 `ensure_project_repo_with_layout`）。
#[derive(Debug, Clone)]
pub enum ExistingRepoLayoutState {
    /// private git_dir 和 worktree/.git 都没有 repo。
    NotGitRepo,
    /// private git_dir 已有 repo（或已从 worktree/.git 迁移过来），可正常打开。
    Ready(GitRepoLayout),
}

/// #644 评论 5493295108 问题2：解析/迁移已有仓库到 layout 指定的 git_dir。
///
/// 语义：
/// - private git_dir 已有 repo → 返回 `Ready(layout)`
/// - private 没有 + worktree/.git 有 repo → 迁移后返回 `Ready(layout)`
/// - 两边都没有 → 返回 `NotGitRepo`
///
/// 与 `ensure_project_repo_with_layout` 的区别：本函数不 init 新仓库，
/// 只处理"已有仓库位置"。App target 用这个，避免在 App data root 下
/// 误 init 一个新仓库。
pub fn resolve_existing_repo_layout(
    layout: &GitRepoLayout,
) -> crate::Result<ExistingRepoLayoutState> {
    crate::storage::git_runtime::ensure_initialized()?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    // 1. private git_dir 已有仓库 → 幂等返回 Ready。
    if git2::Repository::open(&layout.git_dir).is_ok() {
        // 如果 embedded .git 同时存在，尝试完成迁移清理。
        if default_git_dir.exists() && is_external {
            complete_migration_with_journal(
                &layout.git_dir,
                &default_git_dir,
                &layout.worktree_root,
            )?;
        }
        return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
    }

    // 2. worktree_root 有内嵌 .git → 迁移到外部 git_dir，返回 Ready。
    if default_git_dir.exists() && is_external {
        migrate_embedded_git(&default_git_dir, &layout.git_dir, &layout.worktree_root)?;
        return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
    }

    // 3. 两边都没有 → NotGitRepo。
    Ok(ExistingRepoLayoutState::NotGitRepo)
}

/// 打开仓库（从 layout 获取 git2::Repository，设置正确的 workdir）。
///
/// 失败时返回 Err。
pub fn open_repo(layout: &GitRepoLayout) -> std::result::Result<git2::Repository, crate::Error> {
    open_repo_with_layout(layout)
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("open_repo: {}", e))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal_path(git_dir: &Path) -> PathBuf {
        git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME)
    }

    /// 正常迁移：迁移完成后 embedded .git 不存在、private repo 存在、journal 已清理。
    #[test]
    fn migrate_writes_and_clears_journal() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        // 在 worktree_root 下创建 embedded .git。
        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");
        assert!(embedded_git.exists());

        let git_dir = private_root.join("repo.git");
        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());

        ensure_project_repo_with_layout(&layout).unwrap();

        assert!(!embedded_git.exists(), "embedded .git should be removed");
        assert!(git_dir.exists(), "private git_dir should exist");
        assert!(
            !journal_path(&git_dir).exists(),
            "migration journal should be cleaned after success"
        );
    }

    /// 双仓库并存但无 journal：无法证明 ownership，保留 embedded .git，返回 Ok(())。
    #[test]
    fn dual_repo_no_journal_keeps_embedded() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");

        // private git_dir 是独立仓库，不是迁移来的（无 journal）。
        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let result = ensure_project_repo_with_layout(&layout);

        assert!(
            result.is_ok(),
            "no journal should keep embedded .git and return Ok"
        );
        assert!(
            embedded_git.exists(),
            "embedded .git must be preserved without journal"
        );
        assert!(git_dir.exists(), "private git_dir should still exist");
    }

    /// #644 评论 5493295108 问题3：journal 拟留 + claimed_source 已删 + 后来新建 worktree/.git：
    /// 恢复时不应删除后来新建的 .git，只清理 journal（terminal cleanup）。
    #[test]
    fn dual_repo_journal_claimed_source_removed_keeps_later_git() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        // private git_dir 是已迁移好的仓库。
        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        // 后来在 worktree 下新建 .git（别人的仓库）。
        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");

        // 写 journal：claimed_source 指向一个已不存在的路径（迁移时已删），
        // original_source 指向 worktree/.git（现在存在的是别人后来新建的）。
        let journal = LayoutMigrationJournal {
            migration_uuid: uuid::Uuid::new_v4().to_string(),
            worktree_canonical: canonicalize_or_lossy(&worktree_root),
            original_source: embedded_git.to_string_lossy().into_owned(),
            claimed_source: worktree_root
                .join(".git.sujian-migrate-source-dead")
                .to_string_lossy()
                .into_owned(),
            target_git_dir: git_dir.to_string_lossy().into_owned(),
            phase: "copied".to_string(),
        };
        write_migration_journal(&git_dir, &journal).unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let result = ensure_project_repo_with_layout(&layout);

        assert!(
            result.is_ok(),
            "claimed_source removed should keep later-created .git and return Ok"
        );
        assert!(
            embedded_git.exists(),
            "later-created embedded .git must be preserved (not the claimed source)"
        );
        assert!(
            !journal_path(&git_dir).exists(),
            "journal should be cleaned after terminal cleanup"
        );
        assert!(git_dir.exists(), "private git_dir should still exist");
    }

    /// 双仓库并存 + journal worktree 不匹配：保留 embedded .git，返回 Ok(())。
    #[test]
    fn dual_repo_journal_worktree_mismatch_keeps_embedded() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");

        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        // 写 journal，worktree 不匹配（指向不同的 worktree）。
        let journal = LayoutMigrationJournal {
            migration_uuid: uuid::Uuid::new_v4().to_string(),
            worktree_canonical: "/nonexistent/worktree".to_string(),
            original_source: "/nonexistent/source.git".to_string(),
            claimed_source: "/nonexistent/claimed".to_string(),
            target_git_dir: git_dir.to_string_lossy().into_owned(),
            phase: "copied".to_string(),
        };
        write_migration_journal(&git_dir, &journal).unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let result = ensure_project_repo_with_layout(&layout);

        assert!(
            result.is_ok(),
            "worktree mismatch should keep embedded .git and return Ok"
        );
        assert!(
            embedded_git.exists(),
            "embedded .git must be preserved on worktree mismatch"
        );
        assert!(
            journal_path(&git_dir).exists(),
            "journal should be preserved on worktree mismatch"
        );
    }

    /// 双仓库并存 + journal 损坏（无效 JSON）：返回 Err，保留 embedded .git。
    #[test]
    fn dual_repo_corrupt_journal_returns_err() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");

        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        // 写损坏的 journal。
        std::fs::write(journal_path(&git_dir), b"not valid json").unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let result = ensure_project_repo_with_layout(&layout);

        assert!(result.is_err(), "corrupt journal should return Err");
        assert!(
            embedded_git.exists(),
            "embedded .git must be preserved on corrupt journal"
        );
    }

    /// #644 评论 5493295108 问题3：迁移完全成功后 journal 已删除；
    /// 无 embedded .git 时 ensure 不进入双仓库分支，幂等 Ok。
    #[test]
    fn no_embedded_after_migration_idempotent() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        // worktree 下无 embedded .git。
        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        // 无 embedded .git → 不进入双仓库分支，直接 Ok(())。
        let result = ensure_project_repo_with_layout(&layout);
        assert!(result.is_ok(), "no embedded .git should be idempotent Ok");
    }

    /// #644 评论 5493295108 问题2：resolve_existing_repo_layout 语义测试。
    #[test]
    fn resolve_existing_repo_layout_private_ready() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        // private git_dir 已有 repo → Ready。
        let git_dir = private_root.join("repo.git");
        git2::Repository::init_bare(&git_dir).unwrap();

        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let state = resolve_existing_repo_layout(&layout).unwrap();
        assert!(matches!(state, ExistingRepoLayoutState::Ready(_)));
    }

    /// #644 评论 5493295108 问题2：private 没有 + worktree/.git 有 → 迁移后 Ready。
    #[test]
    fn resolve_existing_repo_layout_migrates_embedded() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        // worktree 下有 embedded .git，private 没有。
        git2::Repository::init(&worktree_root).unwrap();
        let embedded_git = worktree_root.join(".git");
        assert!(embedded_git.exists());

        let git_dir = private_root.join("repo.git");
        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let state = resolve_existing_repo_layout(&layout).unwrap();
        assert!(matches!(state, ExistingRepoLayoutState::Ready(_)));
        assert!(!embedded_git.exists(), "embedded should be migrated away");
        assert!(git_dir.exists(), "private git_dir should exist");
    }

    /// #644 评论 5493295108 问题2：两边都没有 → NotGitRepo。
    #[test]
    fn resolve_existing_repo_layout_neither_is_not_git_repo() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let worktree_root = tmp.path().join("worktree");
        let private_root = tmp.path().join("private");
        std::fs::create_dir_all(&worktree_root).unwrap();
        std::fs::create_dir_all(&private_root).unwrap();

        let git_dir = private_root.join("repo.git");
        let layout = GitRepoLayout::with_external_git_dir(worktree_root.clone(), git_dir.clone());
        let state = resolve_existing_repo_layout(&layout).unwrap();
        assert!(matches!(state, ExistingRepoLayoutState::NotGitRepo));
        // 不应 init 新仓库。
        assert!(!git_dir.exists(), "resolve should not init new repo");
    }
}
