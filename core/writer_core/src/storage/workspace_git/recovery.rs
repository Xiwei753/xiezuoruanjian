use std::path::PathBuf;

use crate::storage::git_repo_layout::GitRepoLayout;
use crate::Result;

/// Convert `git2::Error` to `crate::Error`.
fn map_git2_err(e: git2::Error) -> crate::Error {
    crate::Error::Io(std::io::Error::other(format!("git2: {}", e)))
}

/// 恢复 workspace Git 崩溃后的不完整状态。
///
/// 只处理本地 commit/index/HEAD 自己的崩溃恢复。
/// 不重新引入旧 `refs/remotes/*`、staging repo、`.sujian-sync-owner` 那套东西。
///
/// #645 评论 5504296097 问题4：真正实现恢复动作，不再只标记 bool。
///
/// 恢复逻辑：
/// 1. 检查仓库能否正常打开（corrupt → 返回 Err）
/// 2. 检查 HEAD 是否有效（无效 → 找 main/master 分支并 `repo.set_head` 真正修复）
/// 3. 检查 index 是否可读（不可读 → 删除损坏的 index 文件后重新打开/重建）
///
/// 返回的 [`WorkspaceRecoveryResult`] 中的 bool 字段准确反映是否真的执行了恢复动作。
pub fn recover_workspace_crash(layout: &GitRepoLayout) -> Result<WorkspaceRecoveryResult> {
    let mut result = WorkspaceRecoveryResult::default();

    let repo = match crate::storage::git_repo_layout::open_repo(layout) {
        Ok(r) => r,
        Err(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "recover_workspace_crash: cannot open repo: {}",
                e
            ))));
        }
    };

    // 检查 HEAD 是否有效；无效时找 main/master 分支并真正 set_head。
    match repo.head() {
        Ok(_head) => {
            // HEAD is valid and points to a commit — nothing to do.
        }
        Err(_) => {
            // HEAD 无效 — 尝试找 main/master 分支并真正 set_head 修复。
            let branch_ref = find_local_branch_ref(&repo, "main")
                .or_else(|| find_local_branch_ref(&repo, "master"));
            if let Some(refname) = branch_ref {
                repo.set_head(&refname).map_err(map_git2_err)?;
                result.head_was_recovered = true;
                log::info!(
                    "recover_workspace_crash: HEAD was invalid, set_head to {}",
                    refname
                );
            } else {
                log::warn!(
                    "recover_workspace_crash: HEAD invalid and no main/master branch found; \
                     leaving HEAD as-is (unborn branch)"
                );
            }
        }
    }

    // 检查 index 完整性；损坏时重建 index。
    match repo.index() {
        Ok(_index) => {
            // Index is readable — assume it's OK.
        }
        Err(_) => {
            // Index 不可读 — 删除损坏的 index 文件后重新打开（git2 会重建空 index）。
            match rebuild_index(&repo) {
                Ok(()) => {
                    result.index_corrupted = true;
                    log::info!(
                        "recover_workspace_crash: index was corrupted, rebuilt by removing \
                         stale index file"
                    );
                }
                Err(e) => {
                    log::warn!(
                        "recover_workspace_crash: index rebuild failed: {} — \
                         leaving corrupt index for git CLI to handle",
                        e
                    );
                    // 仍标记 index_corrupted，让调用方知道有问题。
                    result.index_corrupted = true;
                }
            }
        }
    }

    Ok(result)
}

/// 查找本地分支的完整 ref 名称（`refs/heads/<name>`），分支不存在时返回 None。
fn find_local_branch_ref(repo: &git2::Repository, name: &str) -> Option<String> {
    let branch = repo.find_branch(name, git2::BranchType::Local).ok()?;
    let ref_name = branch.into_reference().name_bytes().to_vec();
    String::from_utf8(ref_name).ok()
}

/// 重建 index：删除损坏的 index 文件，然后用 `git2::Index::open` 重新打开
/// （git2 会创建一个新的空 index）。如果 HEAD 有效，再 `checkout_head` 把 HEAD tree
/// 写回 index，让 index 反映 HEAD 状态。
fn rebuild_index(repo: &git2::Repository) -> std::result::Result<(), git2::Error> {
    // git2::Repository 没有 index_path() 方法，用 repo.path()（.git 目录）拼接。
    let index_path: PathBuf = repo.path().join("index");
    if index_path.exists() {
        std::fs::remove_file(&index_path).map_err(|e| {
            git2::Error::from_str(&format!(
                "rebuild_index: failed to remove stale index {}: {}",
                index_path.display(),
                e
            ))
        })?;
    }
    // 重新打开 index（git2 会创建新的空 index 文件）。
    let mut new_index = git2::Index::open(&index_path)?;
    // 如果 HEAD 有效，把 HEAD tree 写回 index，让 index 反映 HEAD 状态。
    if let Ok(head) = repo.head() {
        if let Ok(tree) = head.peel_to_tree() {
            new_index.read_tree(&tree)?;
            new_index.write()?;
        }
    }
    Ok(())
}

/// workspace Git 崩溃恢复结果。
#[derive(Debug, Default)]
pub struct WorkspaceRecoveryResult {
    /// HEAD 被恢复到有效分支。
    pub head_was_recovered: bool,
    /// index 不可读（损坏），已尝试重建。
    pub index_corrupted: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::storage::git_repo_layout::GitRepoLayout;

    #[test]
    fn test_recover_clean_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        let result = recover_workspace_crash(&layout).unwrap();
        assert!(!result.head_was_recovered);
        assert!(!result.index_corrupted);
    }

    #[test]
    fn test_recover_with_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        super::super::history::record_workspace_changes(
            &layout,
            &[std::path::PathBuf::from("a.txt")],
            "initial",
        )
        .unwrap();

        let result = recover_workspace_crash(&layout).unwrap();
        assert!(!result.head_was_recovered);
    }

    /// #645 评论 5504296097 问题4：HEAD 无效时真正 set_head 修复。
    #[test]
    fn test_recover_invalid_head_sets_head_to_main() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();

        // 先制造一个 commit（产生 main 分支）。
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        super::super::history::record_workspace_changes(
            &layout,
            &[std::path::PathBuf::from("a.txt")],
            "init",
        )
        .unwrap();

        // 损坏 HEAD：把 HEAD 指向一个不存在的 ref。
        // 注意：symbolic ref 格式必须以 "ref: " 开头，否则 git2 会把它当作
        // loose ref（SHA）解析，报 "corrupted loose reference file"。
        let repo = super::super::repo::open_workspace_repo(&layout).unwrap();
        std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/nonexistent\n").unwrap();
        drop(repo);

        let result = recover_workspace_crash(&layout).unwrap();
        assert!(
            result.head_was_recovered,
            "HEAD 无效时应真正 set_head 到 main"
        );

        // 验证 HEAD 现在指向 main 或 master（默认分支名因系统而异）。
        let repo = super::super::repo::open_workspace_repo(&layout).unwrap();
        let head = repo.head().unwrap();
        let head_name = head
            .symbolic_target()
            .map(|s| s.to_string())
            .or_else(|| head.name().map(|s| s.to_string()))
            .unwrap_or_default();
        assert!(
            head_name.contains("main") || head_name.contains("master"),
            "HEAD 应指向 main 或 master，实际指向: {}",
            head_name
        );
    }
}
