use crate::storage::git_repo_layout::GitRepoLayout;
use crate::Result;

fn map_git2_err(e: git2::Error) -> crate::Error {
    crate::Error::Io(std::io::Error::other(format!("git2: {}", e)))
}

/// 按本地 commit OID 回滚 workspace 到指定状态。
///
/// 使用 `git2::ResetType::Hard` 重置工作树和 index 到目标 commit。
/// 不认识远端 revision、GitHub SHA、remote refs — 只操作本地 commit。
pub fn rollback_to_commit(layout: &GitRepoLayout, target_oid: git2::Oid) -> Result<()> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let commit = repo.find_commit(target_oid).map_err(map_git2_err)?;

    let mut checkout_options = git2::build::CheckoutBuilder::new();
    checkout_options.force();

    repo.reset(
        commit.as_object(),
        git2::ResetType::Hard,
        Some(&mut checkout_options),
    )
    .map_err(map_git2_err)?;

    Ok(())
}

/// 回滚 workspace 到最近一次 commit（HEAD）。
pub fn rollback_to_head(layout: &GitRepoLayout) -> Result<()> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let head = repo.head().map_err(map_git2_err)?;

    let commit = head.peel_to_commit().map_err(map_git2_err)?;

    let mut checkout_options = git2::build::CheckoutBuilder::new();
    checkout_options.force();

    repo.reset(
        commit.as_object(),
        git2::ResetType::Hard,
        Some(&mut checkout_options),
    )
    .map_err(map_git2_err)?;

    Ok(())
}

/// 回滚单个文件到 HEAD 版本（soft 回滚）。
pub fn rollback_file_to_head(layout: &GitRepoLayout, file_path: &std::path::Path) -> Result<()> {
    let repo = crate::storage::git_repo_layout::open_repo(layout)?;

    let head = repo.head().map_err(map_git2_err)?;

    let tree = head.peel_to_tree().map_err(map_git2_err)?;

    let mut checkout_options = git2::build::CheckoutBuilder::new();
    checkout_options.path(file_path);

    repo.checkout_tree(tree.as_object(), Some(&mut checkout_options))
        .map_err(map_git2_err)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn setup_git_user(layout: &GitRepoLayout) {
    let repo = super::super::repo::open_workspace_repo(layout).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test User").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::storage::git_repo_layout::GitRepoLayout;

    #[test]
    fn test_rollback_to_head() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();
        setup_git_user(&layout);

        std::fs::write(tmp.path().join("file.txt"), "version1").unwrap();
        super::super::history::record_workspace_changes(
            &layout,
            &[std::path::PathBuf::from("file.txt")],
            "v1",
        )
        .unwrap();

        std::fs::write(tmp.path().join("file.txt"), "version2").unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
            "version2"
        );

        rollback_to_head(&layout).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
            "version1"
        );
    }

    #[test]
    fn test_rollback_to_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = GitRepoLayout::new(tmp.path().to_path_buf());
        super::super::repo::ensure_workspace_repo(&layout).unwrap();
        setup_git_user(&layout);

        std::fs::write(tmp.path().join("file.txt"), "v1").unwrap();
        let result = super::super::history::record_workspace_changes(
            &layout,
            &[std::path::PathBuf::from("file.txt")],
            "v1",
        )
        .unwrap();
        let v1_oid = result.oid.unwrap();

        std::fs::write(tmp.path().join("file.txt"), "v2").unwrap();
        super::super::history::record_workspace_changes(
            &layout,
            &[std::path::PathBuf::from("file.txt")],
            "v2",
        )
        .unwrap();

        rollback_to_commit(&layout, v1_oid).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("file.txt")).unwrap(),
            "v1"
        );
    }
}
