use std::path::{Path, PathBuf};

use crate::storage::git_repo_layout::GitRepoLayout;
use crate::storage::workspace_git::model::{WorkspaceChangeSet, WorkspaceCommitResult};
use crate::storage::workspace_paths::is_workspace_history_path;
use crate::Result;

/// Convert `git2::Error` to `crate::Error`.
fn map_git2_err(e: git2::Error) -> crate::Error {
    crate::Error::Io(std::io::Error::other(format!("git2: {}", e)))
}

/// 从 index 移除路径，忽略 ENOTFOUND（路径从未 tracked 时 remove_path 返回该错误）。
fn index_remove_path(index: &mut git2::Index, path: &Path) -> Result<()> {
    match index.remove_path(path) {
        Err(e) if e.code() != git2::ErrorCode::NotFound => Err(map_git2_err(e)),
        _ => Ok(()),
    }
}

/// #645 评论 5504296097 问题2(b/c)：按目录前缀从 index 移除所有 tracked entries。
///
/// 遍历 index entries，对路径以前缀开头的条目调 `index.remove_path`。
/// 不重新扫描整个 workspace。前缀可以是目录（`projects/{pid}`）或
/// 带尾斜杠的形式（`projects/{pid}/`），内部统一按字符串前缀匹配。
fn index_remove_tree(index: &mut git2::Index, prefix: &Path) -> Result<()> {
    // #645 评论 5504296097 问题4(c)：统一用 '/' 分隔，避免 Windows 下
    // prefix 'projects\pid' 与 Git index 里的 'projects/pid/...' 匹配不上。
    // Git index 始终用正斜杠存储路径（POSIX 形式），无论平台。
    let prefix_str = prefix.to_string_lossy().replace('\\', "/");
    let prefix_with_slash = format!("{}/", prefix_str);
    // 收集要删除的路径（不在遍历中 mutate index）。
    let to_remove: Vec<PathBuf> = index
        .iter()
        .filter_map(|e| {
            let p = std::str::from_utf8(&e.path).unwrap_or("");
            if p.is_empty() {
                None
            } else if p == prefix_str || p.starts_with(&prefix_with_slash) {
                Some(PathBuf::from(p))
            } else {
                None
            }
        })
        .collect();
    for rel in to_remove {
        index_remove_path(index, &rel)?;
    }
    Ok(())
}

/// #645 评论 5504296097 问题4(a)：清理 index 中不属于 history 的旧 tracked 条目。
///
/// 旧版本可能已经把 secrets/sync engine state stage 进 index。新版本规则升级后，
/// 这些文件不应出现在本地 Git history 的 tree 里。本函数遍历当前 index entries，
/// 对不属于 [`is_workspace_history_path`] 的条目调 [`index_remove_path`]，
/// 保证每次新 commit 的 tree 自动清除旧版本误跟踪的内部文件。不扫描 worktree。
fn purge_non_history_index_entries(index: &mut git2::Index) -> Result<()> {
    let tracked: Vec<PathBuf> = index
        .iter()
        .filter_map(|e| {
            let p = std::str::from_utf8(&e.path).unwrap_or("");
            if p.is_empty() {
                None
            } else {
                Some(PathBuf::from(p))
            }
        })
        .collect();
    for path in tracked {
        if !is_workspace_history_path(&path) {
            index_remove_path(index, &path)?;
        }
    }
    Ok(())
}

/// #645 评论 5504296097 问题2(d)：应用自己的稳定签名。
///
/// 不依赖系统 Git 配置（Android/普通用户环境可能没有全局 Git identity）。
/// 同时在 [`crate::storage::workspace_git::repo::ensure_workspace_repo`]
/// 初始化 repo 时写 repo-local identity，这里直接用 `Signature::now`
/// 也能产生 commit，双重保险。
fn app_signature() -> Result<git2::Signature<'static>> {
    git2::Signature::now("Sujian", "local@sujian.invalid").map_err(map_git2_err)
}

/// #645 评论 5504296097 问题1：在 workspace 中记录**显式 paths** 的变更（本地 commit）。
///
/// `paths` 为 workspace-relative paths。**空 paths 直接返回空结果，
/// 绝不触发全量扫描**——这是与 [`record_all_workspace_changes`] 的关键区别。
/// 返回 [`WorkspaceCommitResult`]，`oid == None` 表示没有变更需要提交。
///
/// 每个 path 先走 [`is_workspace_history_path`] 过滤，被排除的 path 直接跳过。
/// 按 worktree 状态决定 `add_path`（文件仍存在）还是 `remove_path`（已删除）。
///
/// 生产写入、StarMap flush、full-sync commit 全部只走本函数。
/// 真正需要"全量建立/修复本地历史快照"的地方显式调
/// [`record_all_workspace_changes`]，不要再用 `&[]` 做隐藏开关。
pub fn record_workspace_paths(
    layout: &GitRepoLayout,
    paths: &[PathBuf],
    message: &str,
) -> Result<WorkspaceCommitResult> {
    // #645 评论 5504296097 问题1：空 paths 直接返回空结果，绝不触发全量扫描。
    if paths.is_empty() {
        return Ok(WorkspaceCommitResult {
            oid: None,
            staged_count: 0,
        });
    }
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;
    let mut index = repo.index().map_err(map_git2_err)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| crate::Error::Other("repo has no workdir".into()))?
        .to_path_buf();
    let mut staged_count = 0usize;
    for p in paths {
        if !is_workspace_history_path(p) {
            continue;
        }
        let abs = workdir.join(p);
        if abs.exists() {
            index.add_path(p).map_err(map_git2_err)?;
        } else {
            index_remove_path(&mut index, p)?;
        }
        staged_count += 1;
    }
    // #645 评论 5504296097 问题4(a)：显式 paths 路径处理完后，清理 index 中
    // 旧版本误跟踪的内部文件（secrets/sync engine state），保证新 commit 的
    // tree 不再包含这些不应进入本地 history 的条目。不扫描 worktree。
    purge_non_history_index_entries(&mut index)?;
    finalize_commit(&repo, &mut index, message, staged_count)
}

/// #645 评论 5504296097 问题2：在 workspace 中记录 [`WorkspaceChangeSet`] 的变更。
///
/// 按 change 类型分别处理：
/// - `Upsert(path)`：stage 文件（`add_path`）；
/// - `Delete(path)`：`remove_path`；
/// - `DeleteTree(prefix)`：按 prefix `remove_path` 所有 tracked entries。
///
/// 每个 path 先走 [`is_workspace_history_path`] 过滤，被排除的 path 直接跳过。
/// 空变更集直接返回空结果，绝不触发全量扫描。
pub fn record_workspace_change_set(
    layout: &GitRepoLayout,
    change_set: &WorkspaceChangeSet,
    message: &str,
) -> Result<WorkspaceCommitResult> {
    if change_set.is_empty() {
        return Ok(WorkspaceCommitResult {
            oid: None,
            staged_count: 0,
        });
    }
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;
    let mut index = repo.index().map_err(map_git2_err)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| crate::Error::Other("repo has no workdir".into()))?
        .to_path_buf();
    let mut staged_count = 0usize;
    for change in &change_set.changes {
        match change {
            crate::storage::workspace_git::model::WorkspaceHistoryChange::Upsert(p) => {
                if !is_workspace_history_path(p) {
                    continue;
                }
                let abs = workdir.join(p);
                if abs.exists() {
                    index.add_path(p).map_err(map_git2_err)?;
                    staged_count += 1;
                }
            }
            crate::storage::workspace_git::model::WorkspaceHistoryChange::Delete(p) => {
                if !is_workspace_history_path(p) {
                    continue;
                }
                index_remove_path(&mut index, p)?;
                staged_count += 1;
            }
            crate::storage::workspace_git::model::WorkspaceHistoryChange::DeleteTree(prefix) => {
                // #645 评论 5504296097 问题2(b/c)：按 prefix 删除所有 tracked entries。
                // prefix 本身不经过 is_workspace_history_path 过滤——子文件可能
                // 已被该函数排除（如 secrets），但 tracked 的用户内容子文件应被移除。
                // 这里按 prefix 删 index entries，由 index_remove_tree 内部遍历。
                index_remove_tree(&mut index, prefix)?;
                staged_count += 1;
            }
        }
    }
    // #645 评论 5504296097 问题4(a)：change_set 处理完后，清理 index 中
    // 旧版本误跟踪的内部文件（secrets/sync engine state），保证新 commit 的
    // tree 不再包含这些不应进入本地 history 的条目。不扫描 worktree。
    purge_non_history_index_entries(&mut index)?;
    finalize_commit(&repo, &mut index, message, staged_count)
}

/// #645 评论 5504296097 问题1：在 workspace 中记录**全量**变更（本地 commit）。
///
/// 显式全量扫描 worktree，stage 所有非内部文件并移除已删除的 tracked 文件。
/// 只有真正需要"全量建立/修复本地历史快照"的地方（如 bootstrap/recovery/rollback）
/// 才能调用本函数，不要用 `&[]` 做隐藏开关。
pub fn record_all_workspace_changes(
    layout: &GitRepoLayout,
    message: &str,
) -> Result<WorkspaceCommitResult> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;
    let mut index = repo.index().map_err(map_git2_err)?;
    stage_all_with_excludes(&repo, &mut index)?;
    let staged_count = index.iter().count();
    finalize_commit(&repo, &mut index, message, staged_count)
}

/// 写 index、比较 tree、创建 commit 的共用尾段。
fn finalize_commit(
    repo: &git2::Repository,
    index: &mut git2::Index,
    message: &str,
    staged_count: usize,
) -> Result<WorkspaceCommitResult> {
    index.write().map_err(map_git2_err)?;

    let new_tree_oid = index.write_tree().map_err(map_git2_err)?;

    // #645 评论 5504296097 问题2(b)：比较新 tree 和 HEAD tree，无变化不造空 commit。
    let head_tree_oid = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .and_then(|c| c.tree().ok())
        .map(|t| t.id());

    if head_tree_oid == Some(new_tree_oid) {
        return Ok(WorkspaceCommitResult {
            oid: None,
            staged_count: 0,
        });
    }

    let tree = repo.find_tree(new_tree_oid).map_err(map_git2_err)?;

    let parent_oid = repo.head().ok().and_then(|h| h.target());

    let parent_commit = parent_oid
        .map(|oid| repo.find_commit(oid).map_err(map_git2_err))
        .transpose()?;

    let parents: Vec<&git2::Commit> = match &parent_commit {
        Some(p) => vec![p],
        None => vec![],
    };

    let tree_ref: &git2::Tree = &tree;
    let author = app_signature()?;
    let committer = author.clone();

    let new_oid = repo
        .commit(
            Some("HEAD"),
            &author,
            &committer,
            message,
            tree_ref,
            &parents,
        )
        .map_err(map_git2_err)?;

    Ok(WorkspaceCommitResult {
        oid: Some(new_oid),
        staged_count,
    })
}

/// #645 评论 5504296097 问题1：向后兼容的旧入口。
///
/// 保留给 recovery/rollback 测试和旧代码用。**新代码不应再调用本函数**——
/// 显式 paths 请用 [`record_workspace_paths`]，全量请用
/// [`record_all_workspace_changes`]，变更集请用 [`record_workspace_change_set`]。
///
/// 语义：`paths.is_empty()` → 全量扫描（与旧行为一致，避免破坏 recovery/rollback）；
/// 非空 → 显式 paths。
///
/// #645 评论 5504296097 问题4(b)：降级为 `pub(crate)`，不再对外公开。
/// 外部调用方应改用三个明确入口（`record_workspace_paths` /
/// `record_workspace_change_set` / `record_all_workspace_changes`）。
///
/// 当前所有调用点都在 `#[cfg(test)]` 模块内（history/recovery/rollback 的测试），
/// 因此标记为 `#[cfg(test)]`。若将来生产代码需要复用其"空 paths → 全量"语义，
/// 去掉 `#[cfg(test)]` 并改调三个明确入口之一即可。
#[cfg(test)]
pub(crate) fn record_workspace_changes(
    layout: &GitRepoLayout,
    paths: &[PathBuf],
    message: &str,
) -> Result<WorkspaceCommitResult> {
    if paths.is_empty() {
        record_all_workspace_changes(layout, message)
    } else {
        record_workspace_paths(layout, paths, message)
    }
}

/// 查看 workspace 未提交的变更 diff。
///
/// #645 评论 5504296097 问题2(c)：`diff_workspace` 也使用
/// [`is_workspace_history_path`] 过滤，被排除的 path 不出现在 diff 中。
pub fn diff_workspace(layout: &GitRepoLayout) -> Result<Vec<WorkspaceDiffEntry>> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let mut diff_options = git2::DiffOptions::new();
    diff_options.include_untracked(true);
    diff_options.recurse_untracked_dirs(true);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut diff_options))
        .map_err(map_git2_err)?;

    let mut entries = Vec::new();
    for delta_idx in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(delta_idx).ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(format!(
                "diff delta index {} out of range",
                delta_idx
            )))
        })?;
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        // 统一过滤：被排除的 path 不出现在 diff 中。
        if !is_workspace_history_path(&path) {
            continue;
        }
        let status = match delta.status() {
            git2::Delta::Added => WorkspaceDiffStatus::Added,
            git2::Delta::Deleted => WorkspaceDiffStatus::Deleted,
            git2::Delta::Modified => WorkspaceDiffStatus::Modified,
            git2::Delta::Renamed => WorkspaceDiffStatus::Renamed,
            git2::Delta::Typechange => WorkspaceDiffStatus::Typechange,
            _ => WorkspaceDiffStatus::Other,
        };
        entries.push(WorkspaceDiffEntry { path, status });
    }

    Ok(entries)
}

/// 列出 workspace 本地 Git 历史（最近 N 条 commit）。
pub fn list_workspace_history(
    layout: &GitRepoLayout,
    max_count: usize,
) -> Result<Vec<WorkspaceCommitInfo>> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let mut revwalk = repo.revwalk().map_err(map_git2_err)?;
    revwalk.push_head().map_err(map_git2_err)?;
    revwalk
        .set_sorting(git2::Sort::TIME)
        .map_err(map_git2_err)?;

    let mut history = Vec::new();
    for (count, oid_result) in revwalk.enumerate() {
        if count >= max_count {
            break;
        }
        let oid = oid_result.map_err(map_git2_err)?;
        let commit = repo.find_commit(oid).map_err(map_git2_err)?;
        let summary = commit.summary().unwrap_or_default().to_string();
        let time = commit.time().seconds();
        let author_name = commit.author().name().unwrap_or_default().to_string();
        let parent_count = commit.parent_count();
        history.push(WorkspaceCommitInfo {
            oid,
            message: summary,
            timestamp_secs: time,
            author: author_name,
            parent_count,
        });
    }

    Ok(history)
}

/// 排除本地内部文件后 stage 所有变更。
///
/// #645 评论 5504296097 问题2(a)：全量扫描时对比 index 中已有的 tracked paths
/// 和 worktree 中现存的文件，找出已删除的并 `remove_path`。
fn stage_all_with_excludes(repo: &git2::Repository, index: &mut git2::Index) -> Result<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| crate::Error::Other("repo has no workdir".into()))?
        .to_path_buf();

    // 1. 递归 stage 现存文件（跳过内部路径）。
    stage_dir_recursive(&workdir, &workdir, index)?;

    // 2. 扫描 index 中已 tracked 但 worktree 已删除的路径，调 remove_path。
    //    git2::Index::iter() 返回当前 index 的所有条目；对每个条目检查 worktree
    //    是否仍存在，不存在则 remove_path（让 commit 反映删除）。
    let tracked_paths: Vec<PathBuf> = index
        .iter()
        .filter_map(|e| {
            let p = std::str::from_utf8(&e.path).unwrap_or("");
            if p.is_empty() {
                None
            } else {
                Some(PathBuf::from(p))
            }
        })
        .collect();
    for rel in tracked_paths {
        if !is_workspace_history_path(&rel) {
            // #645 评论 5504296097 Blocker 1：规则升级后，旧版本误跟踪的
            // 内部文件（如 secrets.local.json）从新 HEAD tree 里移除。
            // 不能只跳过——跳过会让旧 tracked 条目留在 index 里，凭据继续
            // 出现在后续 commit 的 tree 中。
            index_remove_path(index, &rel)?;
            continue;
        }
        let abs = workdir.join(&rel);
        if !abs.exists() {
            // 文件已被删除 → 从 index 移除。
            index_remove_path(index, &rel)?;
        }
    }

    Ok(())
}

/// 递归 stage 目录，跳过内部排除路径。
///
/// #645 评论 5504296097 问题1：底层规则统一到
/// [`crate::storage::workspace_paths::is_workspace_history_path`]，
/// 不再 `use crate::sync::staging::commit_plan::is_internal_git_artifact`，
/// 消除 `storage/workspace_git -> sync/staging` 反向依赖。
#[allow(clippy::excessive_nesting)]
fn stage_dir_recursive(root: &Path, dir: &Path, index: &mut git2::Index) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(map_io_err)? {
        let entry = entry.map_err(map_io_err)?;
        let path = entry.path();
        if let Ok(rel) = path.strip_prefix(root) {
            if !is_workspace_history_path(rel) {
                continue;
            }
        }
        if path.is_dir() {
            stage_dir_recursive(root, &path, index)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            index.add_path(rel).map_err(map_git2_err)?;
        }
    }
    Ok(())
}

/// workspace diff 状态。
#[derive(Debug, Clone)]
pub enum WorkspaceDiffStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Typechange,
    Other,
}

/// 单条 workspace diff 条目。
#[derive(Debug, Clone)]
pub struct WorkspaceDiffEntry {
    pub path: PathBuf,
    pub status: WorkspaceDiffStatus,
}

/// workspace commit 信息。
#[derive(Debug, Clone)]
pub struct WorkspaceCommitInfo {
    pub oid: git2::Oid,
    pub message: String,
    pub timestamp_secs: i64,
    pub author: String,
    pub parent_count: usize,
}

fn map_io_err(e: std::io::Error) -> crate::Error {
    crate::Error::Io(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::cmp_owned)]
mod tests {
    use super::*;
    use crate::storage::git_repo_layout::GitRepoLayout;

    #[test]
    fn test_record_and_list_history() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        let file1 = tmp.path().join("test.txt");
        std::fs::write(&file1, "hello").unwrap();

        let result =
            record_workspace_changes(&layout, &[PathBuf::from("test.txt")], "first commit")
                .unwrap();
        assert!(result.oid.is_some());
        assert_eq!(result.staged_count, 1);

        let history = list_workspace_history(&layout, 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].message, "first commit");
    }

    #[test]
    fn test_diff_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        std::fs::write(tmp.path().join("new.txt"), "content").unwrap();

        let diffs = diff_workspace(&layout).unwrap();
        assert!(!diffs.is_empty());
        assert!(diffs.iter().any(|d| d.path == PathBuf::from("new.txt")));
    }

    /// #645 评论 5504296097 问题2(b)：无变化不造空 commit。
    #[test]
    fn test_no_changes_returns_none_oid() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let r1 = record_workspace_changes(&layout, &[PathBuf::from("a.txt")], "first").unwrap();
        assert!(r1.oid.is_some());

        // 同一内容再调一次 → 无变化 → oid=None。
        let r2 = record_workspace_changes(&layout, &[PathBuf::from("a.txt")], "noop").unwrap();
        assert!(r2.oid.is_none(), "无变化时应返回 oid=None");
        assert_eq!(r2.staged_count, 0);

        // history 仍只有 1 条。
        let history = list_workspace_history(&layout, 10).unwrap();
        assert_eq!(history.len(), 1);
    }

    /// #645 评论 5504296097 问题2(a)：删除文件用 remove_path，能进入 commit。
    #[test]
    fn test_delete_file_enters_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        std::fs::write(tmp.path().join("del.txt"), "x").unwrap();
        record_workspace_changes(&layout, &[PathBuf::from("del.txt")], "add").unwrap();

        // 删除文件。
        std::fs::remove_file(tmp.path().join("del.txt")).unwrap();
        let r = record_workspace_changes(&layout, &[PathBuf::from("del.txt")], "del").unwrap();
        assert!(r.oid.is_some(), "删除文件应产生 commit");

        // 验证文件在 HEAD tree 中已不存在。
        let repo = super::super::repo::open_workspace_repo(&layout).unwrap();
        let head = repo.head().unwrap();
        let tree = head.peel_to_tree().unwrap();
        assert!(
            tree.get_name("del.txt").is_none(),
            "del.txt 应已从 tree 中删除"
        );
    }

    /// #645 评论 5504296097 问题2(c)：显式 paths 也走过滤。
    #[test]
    fn test_explicit_internal_path_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        // 显式传一个内部路径（.git/HEAD）—— 应被跳过，不产生 commit。
        std::fs::write(tmp.path().join("user.txt"), "x").unwrap();
        record_workspace_changes(&layout, &[PathBuf::from("user.txt")], "u").unwrap();

        let r =
            record_workspace_changes(&layout, &[PathBuf::from(".git/HEAD")], "internal").unwrap();
        // .git/HEAD 被过滤 → 无变化 → oid=None。
        assert!(r.oid.is_none(), "内部路径应被跳过");
    }
}
