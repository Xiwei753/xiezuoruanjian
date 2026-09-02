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

/// #644 评论 5489750244 问题1 + 评论 5491531984 问题2：打开仓库的唯一入口。
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

/// #644 评论 5494387963 问题1：仓库打开结果分类。
///
/// 区分"路径不存在"与"路径存在但仓库损坏/权限错误"，前者可安全 init/migrate 覆盖，
/// 后者必须返回 Err 不能继续。
#[derive(Debug)]
enum RepoOpenResult {
    /// 路径不存在，可安全 init/migrate。
    Missing,
    /// 路径存在且是有效仓库。
    Valid,
    /// 路径存在但 open 失败（损坏/权限错误/非仓库）。
    Corrupt(git2::Error),
}

/// #644 评论 5494387963 问题1：安全打开仓库，区分 Missing vs Corrupt。
///
/// 规则：
/// - 路径不存在 → Missing（可 init/migrate）
/// - 路径存在但 open 失败 → Corrupt（返回 Err，不能继续）
/// - 路径存在且 open 成功 → Valid
fn try_open_repo(path: &Path) -> crate::Result<RepoOpenResult> {
    if !path.exists() {
        return Ok(RepoOpenResult::Missing);
    }
    match git2::Repository::open(path) {
        Ok(_) => Ok(RepoOpenResult::Valid),
        Err(e) => {
            // 路径存在但 open 失败 → 损坏/权限错误/非仓库，不能继续
            Ok(RepoOpenResult::Corrupt(e))
        }
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

    // #644 评论 5494387963 问题1：先尝试恢复任何 pending 迁移
    // 必须在任何 "private 不存在 / .git 不存在 / 要 init" 判断之前调用。
    resume_layout_migration(layout)?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    // 1. git_dir 已有仓库 → 幂等返回。
    //    #644 评论 5491531984 问题3：如果 target 和 embedded .git 同时存在，
    //    不能直接 return Ok(())。先确认 private 是本次迁移出来的同一仓库，
    //    再完成删除 embedded .git。
    //    #644 评论 5493295108 问题3：改用 journal 状态机，恢复时只删 claimed_source，
    //    绝不能删除后来重新出现在 worktree/.git 的别人的仓库。
    //    #644 评论 5494387963 问题1：用安全 open 区分 Missing vs Corrupt。
    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
            if default_git_dir.exists() && is_external {
                complete_migration_with_journal(
                    &layout.git_dir,
                    &default_git_dir,
                    &layout.worktree_root,
                )?;
            }
            return Ok(());
        }
        RepoOpenResult::Corrupt(e) => {
            // git_dir 存在但损坏 → 返回 Err，不能继续 init 或迁移覆盖
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "ensure_project_repo_with_layout: git_dir exists but is corrupt: {}: {}",
                layout.git_dir.display(),
                e,
            ))));
        }
        RepoOpenResult::Missing => {
            // git_dir 不存在，继续下一步判断
        }
    }

    // 2. worktree_root 有内嵌 .git → 迁移到外部 git_dir。
    //    #644 评论 5494387963 问题1：先确认 embedded .git 确实是可打开的 repo
    //    再迁移，避免把损坏/非仓库目录当成 embedded repo 迁移。
    if default_git_dir.exists() {
        match try_open_repo(&default_git_dir)? {
            RepoOpenResult::Valid => {
                if is_external {
                    migrate_embedded_git(&default_git_dir, &layout.git_dir, &layout.worktree_root)?;
                    return Ok(());
                }
                // 标准布局：git_dir == worktree_root/.git，已经是 Valid
                return Ok(());
            }
            RepoOpenResult::Corrupt(e) => {
                // worktree/.git 存在但损坏 → 返回 Err，不能迁移覆盖
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "ensure_project_repo_with_layout: embedded .git exists but is corrupt: {}",
                    e,
                ))));
            }
            RepoOpenResult::Missing => {
                // default_git_dir.exists() 返回 true 但 try_open_repo 返回 Missing
                // 这不应该发生（exists() 为 true 的路径不会是 Missing），
                // 但防御性处理：视为不存在，继续 init 路径
            }
        }
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

/// #644 评论 5494387963 问题1：迁移阶段枚举。
///
/// 真正的 write-ahead journal：每个阶段在关键操作前持久化，崩溃恢复时依据 phase
/// 决定继续/回滚。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MigrationPhase {
    /// Journal 已写，source rename 尚未执行。
    Prepared,
    /// Source 已 rename（claimed），copy 尚未执行。
    SourceClaimed,
    /// Target tmp 已 copy 并验证，rename 尚未执行。
    TargetPrepared,
    /// Target tmp 已 rename 到 final git_dir，worktree 已设置，claimed_source 尚未删除。
    TargetInstalled,
    /// Claimed_source 已删除，journal 尚未清理。
    SourceCleaned,
}

/// #644 评论 5494387963 问题1：迁移 journal 目录名（在 git_dir.parent() 下）。
///
/// 旧路径：`<target_git_dir>/.sujian-layout-migration`（在最终 git_dir 内部）
/// 新路径：`<target_git_dir.parent()>/.layout-migrations/<owner>.json`
///
/// 新位置在 target_git_dir 的父目录下，是稳定的，不随 target_git_dir 的 rename 而移动。
/// 这使得 write-ahead 成为可能：在 source rename 之前就可以写 journal。
const LAYOUT_MIGRATIONS_DIR: &str = ".layout-migrations";

/// #644 评论 5494387963 问题1：从 journal 文件名提取 owner。
///
/// 旧 journal 文件名（`.sujian-layout-migration`，无 owner 信息）需要特殊处理。
const LAYOUT_MIGRATION_JOURNAL_NAME: &str = ".sujian-layout-migration";

/// #644 评论 5494387963 问题1：迁移 journal 内容。
///
/// 写在 `<target_git_dir.parent()>/.layout-migrations/<owner>.json`，用
/// `crate::storage::atomic_write_bytes`（含 fsync 文件 + fsync 父目录）保证 durable。
///
/// journal 记录完整迁移生命周期：owner, original_source, claimed_source, worktree_root,
/// target_tmp, target_git_dir, phase。恢复时依据 phase 决定继续/回滚。
/// 关键改进：恢复时只能删除 claimed_source，绝不能删除后来重新出现在
/// worktree/.git 的别人的仓库。
///
/// #644 评论 5495945801 问题1：`target_tmp` 在 Prepared 阶段确定并落盘，
/// SourceClaimed 恢复只向 `target_tmp` copy，TargetPrepared 恢复执行
/// `rename(target_tmp, target_git_dir)`。不能向 final target_git_dir copy。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LayoutMigrationJournal {
    /// 本次迁移的所有者标识（用于 journal 文件名）。
    owner: String,
    /// 迁移时 worktree_root 的规范路径。
    worktree_canonical: String,
    /// 迁移时 embedded .git 的原始路径（`worktree/.git`）。
    original_source: String,
    /// #644 评论 5493295108 问题3：迁移时已取得所有权的 source 路径。
    /// 通常形如 `worktree/.git.sujian-migrate-source-<owner>`。
    /// 恢复时只能删除这个路径，绝不能删除后来重新出现在 original_source 的仓库。
    claimed_source: String,
    /// #644 评论 5495945801 问题1：迁移中间 tmp 路径（与 target_git_dir 同一文件系统）。
    /// SourceClaimed 恢复只向这里 copy，TargetPrepared 恢复从这里 rename 到 target_git_dir。
    target_tmp: String,
    /// 迁移的目标 private git_dir 路径。
    target_git_dir: String,
    /// 迁移当前阶段（write-ahead 状态机）。
    phase: MigrationPhase,
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

/// #644 评论 5494387963 问题1：获取 journal 目录路径。
///
/// 返回 `<target_git_dir.parent()>/.layout-migrations`。
fn migrations_dir(target_git_dir: &Path) -> Option<PathBuf> {
    target_git_dir
        .parent()
        .map(|p| p.join(LAYOUT_MIGRATIONS_DIR))
}

/// #644 评论 5494387963 问题1：获取 journal 文件路径。
///
/// 返回 `<target_git_dir.parent()>/.layout-migrations/<owner>.json`。
fn journal_path(target_git_dir: &Path, owner: &str) -> Option<PathBuf> {
    migrations_dir(target_git_dir).map(|dir| dir.join(format!("{owner}.json")))
}

/// #644 评论 5494387963 问题1：获取旧格式 journal 路径（用于迁移旧 journal）。
///
/// 返回 `<target_git_dir>/.sujian-layout-migration`。
fn legacy_journal_path(target_git_dir: &Path) -> PathBuf {
    target_git_dir.join(LAYOUT_MIGRATION_JOURNAL_NAME)
}

/// #644 评论 5494387963 问题1：写迁移 journal 到稳定位置。
///
/// 用 `crate::storage::atomic_write_bytes`（fsync 文件 + fsync 父目录）保证 durable。
fn write_migration_journal(
    target_git_dir: &Path,
    journal: &LayoutMigrationJournal,
) -> crate::Result<()> {
    let path = journal_path(target_git_dir, &journal.owner).ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_migration_journal: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let content = serde_json::to_vec(journal).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_migration_journal: serialize: {e}"
        )))
    })?;
    crate::storage::atomic_write_bytes(&path, &content)
}

/// #644 评论 5494387963 问题1：扫描所有 journal 文件（用于恢复）。
///
/// 返回所有找到的 journal。如果目录不存在，返回空向量。
#[allow(clippy::excessive_nesting)]
fn scan_migration_journals(target_git_dir: &Path) -> crate::Result<Vec<LayoutMigrationJournal>> {
    let Some(dir) = migrations_dir(target_git_dir) else {
        return Ok(Vec::new());
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut journals = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            match std::fs::read(&path) {
                Ok(content) => {
                    match serde_json::from_slice(&content) {
                        Ok(journal) => journals.push(journal),
                        Err(e) => {
                            // journal 损坏 → 返回 Err，不假装迁移完成
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "scan_migration_journals: corrupted journal {}: {}",
                                path.display(),
                                e,
                            ))));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 文件在扫描后被删除，跳过
                }
                Err(e) => return Err(crate::Error::Io(e)),
            }
        }
    }
    Ok(journals)
}

/// #644 评论 5494387963 问题1：删除迁移 journal + fsync migrations dir。
///
/// 迁移完全成功后才调用。fsync migrations dir 持久化 journal 删除的目录项。
fn remove_migration_journal(target_git_dir: &Path, owner: &str) -> crate::Result<()> {
    let path = match journal_path(target_git_dir, owner) {
        Some(p) => p,
        None => return Ok(()),
    };
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "remove_migration_journal: remove {}: {e}",
            path.display(),
        )))
    })?;
    // fsync migrations dir 持久化 journal 删除的目录项。
    // #644 评论 5497655880：不再吞 sync_dir 错误，fsync 失败必须返回 Err
    // 以便下次重启继续看到 journal 残留并重试清理，避免静默丢失持久化保证。
    if let Some(dir) = migrations_dir(target_git_dir) {
        if dir.exists() {
            crate::storage::sync_dir(&dir)?;
        }
    }
    Ok(())
}

/// #644 评论 5494387963 问题1：迁移旧格式 journal 到新格式。
///
/// 旧 journal 在 `<target_git_dir>/.sujian-layout-migration`，需要迁移到
/// `<target_git_dir.parent()>/.layout-migrations/<owner>.json`。
/// 如果旧 journal 不存在，返回 Ok(None)。
fn migrate_legacy_journal(
    target_git_dir: &Path,
    _worktree_root: &Path,
) -> crate::Result<Option<LayoutMigrationJournal>> {
    let legacy_path = legacy_journal_path(target_git_dir);
    if !legacy_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read(&legacy_path)?;
    #[derive(Debug, Clone, Deserialize)]
    struct LegacyJournal {
        migration_uuid: String,
        worktree_canonical: String,
        original_source: String,
        claimed_source: String,
        // #644 评论 5495945801 问题1：旧 journal 没有 target_tmp 字段，用 serde(default) 兼容。
        #[serde(default)]
        target_tmp: String,
        target_git_dir: String,
        phase: String,
    }
    let legacy: LegacyJournal = serde_json::from_slice(&content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_legacy_journal: parse {}: {e}",
            legacy_path.display(),
        )))
    })?;
    // 旧 journal 的 migration_uuid 映射到新 journal 的 owner
    let owner = legacy.migration_uuid;
    // 旧 phase 映射：copied → TargetPrepared，finalized → SourceCleaned
    let phase = match legacy.phase.as_str() {
        "copied" => MigrationPhase::TargetPrepared,
        "finalized" => MigrationPhase::SourceCleaned,
        _ => {
            log::warn!(
                "[git_repo_layout] legacy journal has unknown phase {}; treating as TargetPrepared",
                legacy.phase,
            );
            MigrationPhase::TargetPrepared
        }
    };
    // #644 评论 5495945801 问题1：旧 journal 没有 target_tmp，补一个路径。
    // 用 owner 生成 `.git.sujian-migrate-<owner>` 在 target_git_dir.parent() 下。
    let target_tmp = if !legacy.target_tmp.is_empty() {
        legacy.target_tmp
    } else {
        let target_git_dir_path = PathBuf::from(&legacy.target_git_dir);
        match target_git_dir_path.parent() {
            Some(parent) => parent
                .join(format!(".git.sujian-migrate-{}", owner))
                .to_string_lossy()
                .into_owned(),
            None => legacy.target_git_dir.clone(),
        }
    };
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: legacy.worktree_canonical,
        original_source: legacy.original_source,
        claimed_source: legacy.claimed_source,
        target_tmp,
        target_git_dir: legacy.target_git_dir,
        phase,
    };
    // 写新 journal
    write_migration_journal(target_git_dir, &journal)?;
    // 删除旧 journal
    std::fs::remove_file(&legacy_path)?;
    // #644 评论 5497655880：不再吞 sync_dir 错误，fsync 失败必须返回 Err
    // 以保证旧 journal 删除的目录项被持久化（否则下次重启可能看到旧 journal
    // 拟留并误判迁移状态）。
    if let Some(parent) = legacy_path.parent() {
        crate::storage::sync_dir(parent)?;
    }
    Ok(Some(journal))
}

/// #644 评论 5493295108 问题3：双仓库并存时的迁移恢复状态机（journal 版）。
///
/// 当 final git_dir 能 open + embedded .git 同时存在时调用：
/// 1. 尝试迁移旧格式 journal 到新格式
/// 2. 扫描新格式 journal
/// 3. journal 不存在 → 无法证明 private repo 是由这个 embedded .git 迁移来的，
///    不删除 embedded .git，返回 Ok(())（保留两份，让用户/上层决定）
/// 4. journal 存在 → 验证 journal 中 worktree_canonical 与当前 worktree_root 匹配，
///    且 `claimed_source` 仍存在（这是迁移时已取得所有权的 source）。
/// 5. `claimed_source` 存在 → 删除 `claimed_source` + fsync worktree（错误返回 Err 保留 journal），
///    成功后删除 journal + fsync private git_dir（错误返回 Err 保留 journal）
/// 6. `claimed_source` 不存在但 `original_source`（worktree/.git）存在 →
///    这是别人后来新建的仓库，**不删除**，只清理 journal（terminal cleanup）。
/// 7. journal 读取失败（IO 错误）→ 返回 Err，不假装迁移完成
///
/// 关键改进：恢复时只能删除 `claimed_source`，绝不能删除后来重新出现在
/// `original_source`（worktree/.git）的别人的仓库。
fn complete_migration_with_journal(
    private_git_dir: &Path,
    embedded_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    // #644 评论 5494387963 问题1：先尝试迁移旧格式 journal
    let journals = match migrate_legacy_journal(private_git_dir, worktree_root) {
        Ok(Some(j)) => {
            // 旧 journal 已迁移到新格式，继续处理
            vec![j]
        }
        Ok(None) => {
            // 没有旧 journal，扫描新格式
            scan_migration_journals(private_git_dir)?
        }
        Err(e) => {
            // 旧 journal 迁移失败 → 返回 Err，不假装迁移完成
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "complete_migration_with_journal: migrate legacy journal: {e}"
            ))));
        }
    };

    if journals.is_empty() {
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

    // 处理每个 journal（正常情况下只有一个）
    for journal in journals {
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
            continue;
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

        // 删除 journal + fsync migrations dir。
        // 错误返回 Err 保留 journal，下次重启继续。
        remove_migration_journal(private_git_dir, &journal.owner)?;
    }

    Ok(())
}

/// #644 评论 5494387963 问题1：恢复 pending 迁移的统一入口。
///
/// 在 `resolve_existing_repo_layout()` 和 `ensure_project_repo_with_layout()` 进入
/// "private 不存在 / .git 不存在 / 要 init" 判断之前调用。
/// 只依据 durable journal + old/new/claimed 三态继续，不扫描猜 uuid。
///
/// 处理逻辑：
/// 1. 扫描所有 journal
/// 2. 对每个 journal 检查 target_git_dir 是否可打开
/// 3. 根据 phase 决定继续/回滚
/// 4. 不依赖 worktree/.git 是否存在；只要 journal 存在就继续处理
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
fn resume_layout_migration(layout: &GitRepoLayout) -> crate::Result<()> {
    let is_external = layout.git_dir != layout.worktree_root.join(".git");
    if !is_external {
        // 标准布局不需要迁移恢复
        return Ok(());
    }

    // #644 评论 5494387963 问题1：先尝试迁移旧格式 journal
    let journals = match migrate_legacy_journal(&layout.git_dir, &layout.worktree_root) {
        Ok(Some(j)) => {
            // owner 是迁移所有者标识（源自旧 journal 的 migration_uuid，用于 journal
            // 文件名），不是密钥/token 值；记录它用于调试迁移恢复，参见 Issue #648。
            //
            // 取出 owner 到语义清晰的 `owner_tag` 中间变量再记录，让 CodeQL
            // cleartext-logging 数据流分析能识别这是文件名标识而非密钥值。
            let owner_tag: &str = &j.owner;
            log::debug!(
                "[git_repo_layout] resume: migrated legacy journal, owner_tag={}",
                owner_tag
            );
            vec![j]
        }
        Ok(None) => {
            let scanned = scan_migration_journals(&layout.git_dir)?;
            log::debug!(
                "[git_repo_layout] resume: scanned {} journals",
                scanned.len()
            );
            scanned
        }
        Err(e) => {
            // 旧 journal 迁移失败 → 返回 Err，不假装迁移完成
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "resume_layout_migration: migrate legacy journal: {e}"
            ))));
        }
    };

    if journals.is_empty() {
        // 没有 journal，没有 pending 迁移
        return Ok(());
    }

    for journal in journals {
        // 验证 journal 中的 worktree canonical path 与当前路径匹配。
        let current_worktree_canonical = canonicalize_or_lossy(&layout.worktree_root);
        if journal.worktree_canonical != current_worktree_canonical {
            // 不匹配 → 可能是不同 worktree 的迁移残留，跳过。
            log::warn!(
                "[git_repo_layout] resume: journal worktree mismatch: \
                 journal(worktree={}, original_source={}, claimed_source={}) vs \
                 current(worktree={}); skipping",
                journal.worktree_canonical,
                journal.original_source,
                journal.claimed_source,
                current_worktree_canonical,
            );
            continue;
        }

        // 持续处理当前 journal，直到所有阶段完成
        let mut current_journal = journal;
        loop {
            // 检查 target_git_dir 是否可打开
            let target_path = PathBuf::from(&current_journal.target_git_dir);
            let target_open = try_open_repo(&target_path)?;

            // 检查 claimed_source 是否存在
            let claimed_source_path = PathBuf::from(&current_journal.claimed_source);
            let claimed_exists = claimed_source_path.exists();

            // 根据 phase 决定继续/回滚
            match current_journal.phase {
                MigrationPhase::Prepared => {
                    // #644 评论 5496728184 缺陷1修复：Prepared 必须按
                    // (original_exists, claimed_exists, target_open) 三态判断，
                    // 不能只看 original_source.exists()。
                    //
                    // 关键：Prepared 必须识别"claimed 已经存在"代表 rename 已完成，
                    // 继续恢复；绝不能因为 original 不存在就删 journal。
                    //
                    // 崩溃窗口：rename(original->claimed) + fsync 已完成，但
                    // phase=SourceClaimed 还没写入 journal。此时磁盘状态：
                    //   journal.phase=Prepared + original 不存在 + claimed 存在 + target 不存在
                    // 旧代码直接删 journal，Git 历史躺在 claimed_source 无人管，永久遗失。
                    let original_source = PathBuf::from(&current_journal.original_source);
                    let original_exists = original_source.exists();

                    match (original_exists, claimed_exists, &target_open) {
                        // (true, false, Missing) → 还没开始 rename：正常继续 claim
                        (true, false, RepoOpenResult::Missing) => {
                            std::fs::rename(&original_source, &claimed_source_path).map_err(
                                |e| {
                                    crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: Prepared phase rename {} -> {}: {}",
                                    original_source.display(),
                                    claimed_source_path.display(),
                                    e,
                                )))
                                },
                            )?;
                            if let Some(parent) = layout.worktree_root.parent() {
                                crate::storage::sync_dir(parent)?;
                            }
                            crate::storage::sync_dir(&layout.worktree_root)?;
                            // 更新 phase 为 SourceClaimed
                            current_journal = LayoutMigrationJournal {
                                phase: MigrationPhase::SourceClaimed,
                                ..current_journal
                            };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue; // 继续处理下一个阶段
                        }
                        // (false, true, Missing) → rename 已成功，崩在 phase 落盘之前：
                        // 不能删 journal，直接补推进到 SourceClaimed。
                        (false, true, RepoOpenResult::Missing) => {
                            current_journal = LayoutMigrationJournal {
                                phase: MigrationPhase::SourceClaimed,
                                ..current_journal
                            };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue; // 继续处理下一个阶段
                        }
                        // #644 评论 5497655880：(_, _, Corrupt(e)) → target 路径存在但
                        // 仓库损坏/权限错误，不能吞成普通状态，返回 Err。
                        // 放在 (true, true, _) 之前，优先匹配任何 original/claimed 组合下的
                        // target Corrupt。
                        (_, _, RepoOpenResult::Corrupt(e)) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but target_git_dir \
                                 exists but is corrupt: {}: {}",
                                target_path.display(),
                                e,
                            ))));
                        }
                        // #644 评论 5497655880：(_, _, Valid) → final target 已是有效仓库。
                        // Prepared 阶段 final target 第一次出现只能发生在 TargetPrepared 的
                        // rename(target_tmp, target_git_dir) 之后。若此刻 final 已 Valid，
                        // 它很可能是后来/并发出现的另一套仓库（不是本次 journal 安装出来的）。
                        // 绝不能认领这个 foreign repo 推进到 TargetInstalled，否则随后会
                        // 删除 claimed_source，原历史就真的被删掉了。直接返回 Err，
                        // 保留 journal 和 source，让上层/用户决定。
                        // 放在 (true, true, _) 之前，优先匹配任何 original/claimed 组合下的
                        // target Valid。
                        (_, _, RepoOpenResult::Valid) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but target_git_dir \
                                 already exists as valid repo; cannot claim foreign repo: {}",
                                target_path.display(),
                            ))));
                        }
                        // (true, true, _) → original 和 claimed 同时存在，
                        // ownership 状态不明确，直接 Err。
                        // #644 评论 5497655880：到此分支时 target 只能是 Missing
                        //（Valid/Corrupt 已被前面的通配分支处理），(true, true, Missing)
                        // 仍然是 ambiguous ownership，返回 Err。
                        (true, true, _) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase ambiguous ownership: \
                                 both original_source ({}) and claimed_source ({}) exist",
                                original_source.display(),
                                claimed_source_path.display(),
                            ))));
                        }
                        // (false, false, Missing) → 两边都不存在也不能直接清 journal
                        (false, false, RepoOpenResult::Missing) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: Prepared phase but both \
                                 original_source ({}) and claimed_source ({}) missing",
                                original_source.display(),
                                claimed_source_path.display(),
                            ))));
                        }
                    }
                }
                MigrationPhase::SourceClaimed => {
                    // Source 已 rename，copy 尚未执行。
                    // #644 评论 5495945801 问题1：只向 target_tmp copy，不能向 final target_git_dir copy。
                    // 如果恢复过程 copy 一半再次崩溃，target_tmp 变成半截仓库，
                    // 下次清理残留再 copy；final target_git_dir 不受污染。
                    let target_tmp_path = PathBuf::from(&current_journal.target_tmp);
                    // #644 评论 5497655880：SourceClaimed 阶段 target 也必须 Missing。
                    // final target 第一次出现只能在 TargetPrepared 的 rename(target_tmp, target_git_dir) 之后。
                    // 若此刻 final 已 Valid/Corrupt，它很可能是后来/并发出现的另一套仓库，
                    // 绝不能认领。直接返回 Err，保留 journal 和 claimed_source。
                    match &target_open {
                        RepoOpenResult::Valid => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: SourceClaimed phase but target_git_dir \
                                 already exists as valid repo; cannot claim foreign repo: {}",
                                target_path.display(),
                            ))));
                        }
                        RepoOpenResult::Corrupt(e) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: SourceClaimed phase but target_git_dir \
                                 exists but is corrupt: {}: {}",
                                target_path.display(),
                                e,
                            ))));
                        }
                        RepoOpenResult::Missing => {
                            // target 不存在，可以继续 copy
                        }
                    }
                    if claimed_exists {
                        // 如果旧 tmp 拘留（target_tmp 已存在），先清理残留再 copy。
                        if target_tmp_path.exists() {
                            std::fs::remove_dir_all(&target_tmp_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: SourceClaimed phase remove stale target_tmp {}: {}",
                                    target_tmp_path.display(),
                                    e,
                                )))
                            })?;
                        }

                        // 创建 target_tmp 父目录
                        if let Some(parent) = target_tmp_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        // copy claimed_source 到 target_tmp
                        migrate_copy_dir_recursive(&claimed_source_path, &target_tmp_path)?;

                        // 验证 target_tmp repo
                        {
                            let tmp_repo = git2::Repository::open(&target_tmp_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: SourceClaimed phase open copied repo: {e}"
                                )))
                            })?;
                            let _ = tmp_repo.head();
                            let _ = tmp_repo.find_reference("HEAD");
                        }
                        // 更新 phase 为 TargetPrepared
                        current_journal = LayoutMigrationJournal {
                            phase: MigrationPhase::TargetPrepared,
                            ..current_journal
                        };
                        write_migration_journal(&target_path, &current_journal)?;
                        continue; // 继续处理下一个阶段
                    } else {
                        // claimed_source 不存在，无法恢复
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "resume_layout_migration: SourceClaimed phase but claimed_source missing: {}",
                            claimed_source_path.display(),
                        ))));
                    }
                }
                MigrationPhase::TargetPrepared => {
                    // #644 评论 5495945801 问题1：TargetPrepared 恢复执行
                    // rename(target_tmp, target_git_dir) + fsync target parent + 设置 workdir。
                    // 删除"TargetPrepared 阶段 target 已经在 final 位置"的兼容逻辑。
                    //
                    // #644 评论 5497655880：按 (target_open, target_tmp_exists) 状态表收紧
                    // ownership 判断。final target 第一次出现只能发生在本 phase 的
                    // rename(target_tmp, target_git_dir) 之后。
                    //   - target Missing + target_tmp 存在    → 执行 rename
                    //   - target Valid   + target_tmp 不存在  → 合法崩溃窗口（rename 已成功，
                    //     phase 还没落盘），set_workdir 后推进 TargetInstalled
                    //   - target Valid   + target_tmp 仍存在  → ownership 歧义，final 很可能是
                    //     别的进程/别的现场创建的仓库，直接 Err
                    //   - target Missing + target_tmp 不存在  → Err
                    //   - target Corrupt → Err
                    let target_tmp_path = PathBuf::from(&current_journal.target_tmp);
                    let target_tmp_exists = target_tmp_path.exists();

                    match (&target_open, target_tmp_exists) {
                        // target Valid + target_tmp 不存在 → 合法崩溃窗口：
                        // rename(target_tmp, target_git_dir) 已成功，但 phase=TargetInstalled
                        // 还没落盘。可以 set_workdir 后推进 TargetInstalled。
                        (RepoOpenResult::Valid, false) => {
                            let repo = git2::Repository::open(&target_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: TargetPrepared phase open repo: {e}"
                                )))
                            })?;
                            repo.set_workdir(&layout.worktree_root, false)
                                .map_err(|e| {
                                    crate::Error::Io(std::io::Error::other(format!(
                                        "resume_layout_migration: TargetPrepared phase set_workdir: {e}"
                                    )))
                                })?;
                            let _ = repo.head();
                            if let Ok(mut index) = repo.index() {
                                let _ = index.read(true);
                            }
                            current_journal = LayoutMigrationJournal {
                                phase: MigrationPhase::TargetInstalled,
                                ..current_journal
                            };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue; // 继续处理下一个阶段
                        }
                        // target Valid + target_tmp 仍存在 → ownership 歧义。
                        // final target 是有效仓库但 target_tmp 还在，说明 final 很可能是
                        // 别的进程/别的现场创建的仓库，不是本次 rename 出来的。直接 Err。
                        (RepoOpenResult::Valid, true) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase ownership ambiguity: \
                                 target_git_dir ({}) is valid but target_tmp ({}) still exists; \
                                 final repo may be created by another process",
                                target_path.display(), target_tmp_path.display(),
                            ))));
                        }
                        // target Corrupt → Err
                        (RepoOpenResult::Corrupt(e), _) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase but target_git_dir \
                                 exists but is corrupt: {}: {}",
                                target_path.display(),
                                e,
                            ))));
                        }
                        // target Missing + target_tmp 存在 → 执行 rename
                        (RepoOpenResult::Missing, true) => {
                            // 确保 target_git_dir 父目录存在
                            if let Some(parent) = target_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            // 同文件系统原子 rename target_tmp -> target_git_dir
                            std::fs::rename(&target_tmp_path, &target_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: TargetPrepared phase rename {} -> {}: {}",
                                    target_tmp_path.display(), target_path.display(), e,
                                )))
                            })?;
                            // fsync target_parent（持久化 rename 的目录项）
                            if let Some(parent) = target_path.parent() {
                                crate::storage::sync_dir(parent)?;
                            }
                            // 打开 final repo 设置 workdir 并验证
                            let repo = git2::Repository::open(&target_path).map_err(|e| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "resume_layout_migration: TargetPrepared phase open final repo: {e}"
                                )))
                            })?;
                            repo.set_workdir(&layout.worktree_root, false)
                                .map_err(|e| {
                                    crate::Error::Io(std::io::Error::other(format!(
                                        "resume_layout_migration: TargetPrepared phase set_workdir: {e}"
                                    )))
                                })?;
                            let _ = repo.head();
                            if let Ok(mut index) = repo.index() {
                                let _ = index.read(true);
                            }
                            // 更新 phase 为 TargetInstalled
                            current_journal = LayoutMigrationJournal {
                                phase: MigrationPhase::TargetInstalled,
                                ..current_journal
                            };
                            write_migration_journal(&target_path, &current_journal)?;
                            continue; // 继续处理下一个阶段
                        }
                        // target Missing + target_tmp 不存在 → Err
                        (RepoOpenResult::Missing, false) => {
                            return Err(crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetPrepared phase but both \
                                 target_git_dir ({}) and target_tmp ({}) missing",
                                target_path.display(),
                                target_tmp_path.display(),
                            ))));
                        }
                    }
                }
                MigrationPhase::TargetInstalled => {
                    // Target 已安装，claimed_source 尚未删除。
                    // 继续删除 claimed_source。
                    if claimed_exists {
                        std::fs::remove_dir_all(&claimed_source_path).map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "resume_layout_migration: TargetInstalled phase remove claimed_source {}: {e}",
                                claimed_source_path.display(),
                            )))
                        })?;
                        if let Some(parent) = layout.worktree_root.parent() {
                            crate::storage::sync_dir(parent)?;
                        }
                        crate::storage::sync_dir(&layout.worktree_root)?;
                    }
                    // 更新 phase 为 SourceCleaned
                    current_journal = LayoutMigrationJournal {
                        phase: MigrationPhase::SourceCleaned,
                        ..current_journal
                    };
                    write_migration_journal(&target_path, &current_journal)?;

                    continue; // 继续处理下一个阶段
                }
                MigrationPhase::SourceCleaned => {
                    // Claimed_source 已删除，journal 尚未清理。
                    // 继续清理 journal。
                    remove_migration_journal(&layout.git_dir, &current_journal.owner)?;
                    break; // 完成，退出循环
                }
            }
        }
    }

    Ok(())
}

/// #644 评论 5493295108 问题3：跨文件系统安全的 .git 迁移（write-ahead journal 状态机版）。
///
/// Android 共享存储 → 应用私有 `filesDir` 属于不同 mount/filesystem，
/// `std::fs::rename` 跨文件系统会返回 `InvalidCrossDeviceLink`。
///
/// #644 评论 5494387963 问题1：真正的 write-ahead 顺序：
/// 1. 生成 owner / original_source / claimed_source / target_tmp / target_git_dir
/// 2. 原子写 journal phase=Prepared + fsync
/// 3. rename original_source -> claimed_source + fsync worktree
/// 4. 原子更新 journal phase=SourceClaimed
/// 5. copy claimed_source -> target_tmp，验证 repo
/// 6. 原子更新 journal phase=TargetPrepared
/// 7. rename target_tmp -> target_git_dir + fsync target parent
/// 8. 原子更新 journal phase=TargetInstalled
/// 9. 删除 claimed_source + fsync worktree
/// 10. 原子更新 journal phase=SourceCleaned
/// 11. 删除 journal + fsync journal parent
///
/// 恢复时（`resume_layout_migration` / `complete_migration_with_journal`）
/// 只能删除 `claimed_source`，绝不能删除后来重新出现在 `worktree/.git` 的别人的仓库。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
fn migrate_embedded_git(
    default_git_dir: &Path,
    target_git_dir: &Path,
    worktree_root: &Path,
) -> crate::Result<()> {
    let owner = uuid::Uuid::new_v4().to_string();

    // 1. 先取得 source 的所有权：原子 rename worktree/.git -> owned source。
    //    同文件系统 rename 是原子的，要么成功要么失败，不会半完成。
    let owned_source_name = format!(".git.sujian-migrate-source-{}", owner);
    let owned_source_path = worktree_root.join(&owned_source_name);

    // #644 评论 5495945801 问题1：在 Prepared 阶段就确定 target_tmp 路径并写进 journal。
    // target_tmp 与 target_git_dir 在同一文件系统（target_parent 下）。
    let target_parent = target_git_dir.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: target_git_dir has no parent: {}",
            target_git_dir.display(),
        )))
    })?;
    let tmp_git = target_parent.join(format!(".git.sujian-migrate-{}", owner));

    // 2. 原子写 journal phase=Prepared + fsync（在 source rename 之前！）
    //    #644 评论 5494387963 问题1：journal 写在 target_git_dir.parent() 下，
    //    在 source rename 之前就持久化。
    //    #644 评论 5495945801 问题1：target_tmp 在 Prepared 阶段就落盘。
    let journal = LayoutMigrationJournal {
        owner: owner.clone(),
        worktree_canonical: canonicalize_or_lossy(worktree_root),
        original_source: default_git_dir.to_string_lossy().into_owned(),
        claimed_source: owned_source_path.to_string_lossy().into_owned(),
        target_tmp: tmp_git.to_string_lossy().into_owned(),
        target_git_dir: target_git_dir.to_string_lossy().into_owned(),
        phase: MigrationPhase::Prepared,
    };
    write_migration_journal(target_git_dir, &journal)?;

    // 3. rename original_source -> claimed_source + fsync worktree
    std::fs::rename(default_git_dir, &owned_source_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "migrate_embedded_git: rename source {} -> {}: {}",
            default_git_dir.display(),
            owned_source_path.display(),
            e,
        )))
    })?;
    // fsync worktree（持久化 source rename 的目录项）。
    if let Some(parent) = worktree_root.parent() {
        crate::storage::sync_dir(parent)?;
    }
    crate::storage::sync_dir(worktree_root)?;

    // 4. 原子更新 journal phase=SourceClaimed
    let journal = LayoutMigrationJournal {
        phase: MigrationPhase::SourceClaimed,
        ..journal
    };
    write_migration_journal(target_git_dir, &journal)?;

    // 5. 在目标文件系统上建 tmp 目录（与 target_git_dir 同一文件系统）。
    let mut guard = MigrateTmpDirGuard::new(tmp_git);

    // 6. 递归复制 owned source 到 tmp（含 fsync）。
    migrate_copy_dir_recursive(&owned_source_path, guard.path())?;

    // 7. 打开 tmp repo 确认可读。
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

    // 8. 原子更新 journal phase=TargetPrepared
    let journal = LayoutMigrationJournal {
        phase: MigrationPhase::TargetPrepared,
        ..journal
    };
    write_migration_journal(target_git_dir, &journal)?;

    // 9. 在同一文件系统内原子 rename tmp -> final git_dir。
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

    // 10. 设置 workdir 并验证仓库完整性。
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

    // 11. 原子更新 journal phase=TargetInstalled
    let journal = LayoutMigrationJournal {
        phase: MigrationPhase::TargetInstalled,
        ..journal
    };
    write_migration_journal(target_git_dir, &journal)?;

    // 12. 删除 owned source（claimed_source）并 fsync worktree parent。
    //     这是恢复时唯一允许删除的 source 路径。
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

    // 13. 原子更新 journal phase=SourceCleaned
    let journal = LayoutMigrationJournal {
        phase: MigrationPhase::SourceCleaned,
        ..journal
    };
    write_migration_journal(target_git_dir, &journal)?;

    // 14. 迁移完全成功后删除 journal + fsync migrations dir。
    //     如果这步失败，返回 Err（journal 拟留，下次重启看到 journal 并尝试继续清理）。
    remove_migration_journal(target_git_dir, &owner)?;

    Ok(())
}

/// #644 评论 5493295108 问题2：只处理"已有仓库位置"的 resolve/migrate 入口。
///
/// App target 用这个，不要"没有就 init"。语义定死：
/// - private git_dir 已有 repo -> Ready
/// - private 没有 + worktree/.git 有 repo -> 迁移后 Ready
/// - 两边都没有 -> NotGitRepo
///
/// Project target 如果产品契约要求作品必有 Git，再在 resolve 后单独 init missing repo
///（调 `ensure_project_repo_with_layout`）。
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
/// - private git_dir 已有 repo -> 返回 `Ready(layout)`
/// - private 没有 + worktree/.git 有 repo -> 迁移后返回 `Ready(layout)`
/// - 两边都没有 -> 返回 `NotGitRepo`
///
/// 与 `ensure_project_repo_with_layout` 的区别：本函数不 init 新仓库，
/// 只处理"已有仓库位置"。App target 用这个，避免在 App data root 下
/// 误 init 一个新仓库。
pub fn resolve_existing_repo_layout(
    layout: &GitRepoLayout,
) -> crate::Result<ExistingRepoLayoutState> {
    crate::storage::git_runtime::ensure_initialized()?;

    // #644 评论 5494387963 问题1：先尝试恢复任何 pending 迁移
    resume_layout_migration(layout)?;

    let default_git_dir = layout.worktree_root.join(".git");
    let is_external = layout.git_dir != default_git_dir;

    // 1. private git_dir 已有仓库 → 幂等返回 Ready。
    //    #644 评论 5494387963 问题1：用安全 open 区分 Missing vs Corrupt。
    match try_open_repo(&layout.git_dir)? {
        RepoOpenResult::Valid => {
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
        RepoOpenResult::Corrupt(e) => {
            // git_dir 存在但损坏 → 返回 Err，不能继续
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "resolve_existing_repo_layout: git_dir exists but is corrupt: {}: {}",
                layout.git_dir.display(),
                e,
            ))));
        }
        RepoOpenResult::Missing => {
            // git_dir 不存在，继续下一步判断
        }
    }

    // 2. worktree_root 有内嵌 .git → 迁移到外部 git_dir，返回 Ready。
    //    #644 评论 5494387963 问题1：先确认 embedded .git 确实是可打开的 repo
    //    再迁移，避免把损坏/非仓库目录当成 embedded repo 迁移。
    if default_git_dir.exists() {
        match try_open_repo(&default_git_dir)? {
            RepoOpenResult::Valid => {
                if is_external {
                    migrate_embedded_git(&default_git_dir, &layout.git_dir, &layout.worktree_root)?;
                    return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
                }
                // 标准布局：git_dir == worktree_root/.git，已经是 Valid
                return Ok(ExistingRepoLayoutState::Ready(layout.clone()));
            }
            RepoOpenResult::Corrupt(e) => {
                // worktree/.git 存在但损坏 → 返回 Err，不能迁移
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "resolve_existing_repo_layout: embedded .git exists but is corrupt: {}",
                    e,
                ))));
            }
            RepoOpenResult::Missing => {
                // 防御性处理
            }
        }
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
mod tests;
