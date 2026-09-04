use std::path::{Path, PathBuf};

use crate::storage::git_repo_layout::GitRepoLayout;
use crate::storage::workspace_git::model::WorkspaceCommitResult;
use crate::Result;

/// Convert `git2::Error` to `crate::Error`.
fn map_git2_err(e: git2::Error) -> crate::Error {
    crate::Error::Io(std::io::Error::other(format!("git2: {}", e)))
}

/// 在 workspace 中记录变更（本地 commit）。
///
/// `paths` 为 workspace-relative paths（为空时 stage 所有变更）。
/// `message` 为 commit message。
/// 返回 [`WorkspaceCommitResult`]，`oid == None` 表示没有变更需要提交。
///
/// Stage 排除：secrets、cache、log、runtime、`full-sync-staging/`、
/// `app-meta/transactions/` 等本地内部文件。
pub fn record_workspace_changes(
    layout: &GitRepoLayout,
    paths: &[PathBuf],
    message: &str,
) -> Result<WorkspaceCommitResult> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let mut index = repo.index().map_err(map_git2_err)?;

    if paths.is_empty() {
        stage_all_with_excludes(&repo, &mut index)?;
    } else {
        for p in paths {
            index.add_path(p).map_err(map_git2_err)?;
        }
    }

    index.write().map_err(map_git2_err)?;

    let oid = index.write_tree().map_err(map_git2_err)?;
    let tree = repo.find_tree(oid).map_err(map_git2_err)?;

    let parent_oid = repo.head().ok().and_then(|h| h.target());

    let parent_commit = parent_oid
        .map(|oid| repo.find_commit(oid).map_err(map_git2_err))
        .transpose()?;

    let parents: Vec<&git2::Commit> = match &parent_commit {
        Some(p) => vec![p],
        None => vec![],
    };

    let tree_ref: &git2::Tree = &tree;
    let author = repo.signature().map_err(map_git2_err)?;
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

    let staged_count = if paths.is_empty() {
        index.iter().count()
    } else {
        paths.len()
    };

    Ok(WorkspaceCommitResult {
        oid: Some(new_oid),
        staged_count,
    })
}

/// 查看 workspace 未提交的变更 diff。
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
        let status = match delta.status() {
            git2::Delta::Added => WorkspaceDiffStatus::Added,
            git2::Delta::Deleted => WorkspaceDiffStatus::Deleted,
            git2::Delta::Modified => WorkspaceDiffStatus::Modified,
            git2::Delta::Renamed => WorkspaceDiffStatus::Renamed,
            git2::Delta::Typechange => WorkspaceDiffStatus::Typechange,
            _ => WorkspaceDiffStatus::Other,
        };
        entries.push(WorkspaceDiffEntry {
            path: delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            status,
        });
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
fn stage_all_with_excludes(repo: &git2::Repository, index: &mut git2::Index) -> Result<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| crate::Error::Other("repo has no workdir".into()))?
        .to_path_buf();

    stage_dir_recursive(&workdir, &workdir, index)?;
    Ok(())
}

/// 递归 stage 目录，跳过内部排除路径。
#[allow(clippy::excessive_nesting)]
fn stage_dir_recursive(root: &Path, dir: &Path, index: &mut git2::Index) -> Result<()> {
    use crate::sync::staging::commit_plan::is_internal_git_artifact;

    for entry in std::fs::read_dir(dir).map_err(map_io_err)? {
        let entry = entry.map_err(map_io_err)?;
        let path = entry.path();
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_str = rel.to_string_lossy().to_string();
            if is_internal_git_artifact(&rel_str)
                || rel_str.starts_with("full-sync-staging")
                || rel_str.starts_with("app-meta/transactions")
                || rel_str.contains("secrets")
                || rel_str.starts_with("app-meta/logs")
                || rel_str.contains("cache")
                || rel_str.ends_with(".tmp")
                || rel_str.ends_with(".lock")
            {
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

        // git2 requires user.name and user.email for commits
        let repo = super::super::repo::open_workspace_repo(&layout).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Test User").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        drop(repo);

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
}
