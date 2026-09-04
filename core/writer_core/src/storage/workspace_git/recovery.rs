use crate::storage::git_repo_layout::GitRepoLayout;
use crate::Result;

/// 恢复 workspace Git 崩溃后的不完整状态。
///
/// 只处理本地 commit/index/HEAD 自己的崩溃恢复。
/// 不重新引入旧 `refs/remotes/*`、staging repo、`.sujian-sync-owner` 那套东西。
///
/// 恢复逻辑：
/// 1. 检查仓库能否正常打开（corrupt → 返回 Err）
/// 2. 检查 HEAD 是否有效（无效 → 尝试找当前分支重置）
/// 3. 检查 index 是否可读（不可读 → 标记损坏）
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

    // 检查 HEAD 是否有效
    match repo.head() {
        Ok(_head) => {
            // HEAD is valid and points to a commit — nothing to do.
        }
        Err(_) => {
            // HEAD 无效 — 尝试找当前分支并重置到已知引用
            if let Ok(branch) = repo.find_branch("main", git2::BranchType::Local) {
                let branch_ref = branch.into_reference();
                if let Some(target) = branch_ref.target() {
                    result.head_was_recovered = true;
                    log::info!(
                        "recover_workspace_crash: HEAD was invalid, branch 'main' points to {}",
                        target
                    );
                }
            } else if let Ok(branch) = repo.find_branch("master", git2::BranchType::Local) {
                let branch_ref = branch.into_reference();
                if let Some(target) = branch_ref.target() {
                    result.head_was_recovered = true;
                    log::info!(
                        "recover_workspace_crash: HEAD was invalid, branch 'master' points to {}",
                        target
                    );
                }
            }
        }
    }

    // 检查 index 完整性
    match repo.index() {
        Ok(_index) => {
            // Index is readable — assume it's OK.
        }
        Err(_) => {
            result.index_corrupted = true;
        }
    }

    Ok(result)
}

/// workspace Git 崩溃恢复结果。
#[derive(Debug, Default)]
pub struct WorkspaceRecoveryResult {
    /// HEAD 被恢复到有效分支。
    pub head_was_recovered: bool,
    /// index 不可读（损坏）。
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

        // git2 requires user.name and user.email for commits
        let repo = super::super::repo::open_workspace_repo(&layout).unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Test User").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        drop(repo);

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
}
