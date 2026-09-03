use super::*;
use tempfile::TempDir;

/// live 不是 Git repo：seed_from_live_as_git_repo 等价于 seed_from_live，
/// staging 不应包含 .git/。
#[test]
fn git_staging_non_repo_keeps_staging_non_repo() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(live.join("sub")).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    fs::write(live.join("sub/b.md"), "world").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone(), "git".to_string(), None).unwrap();
    let result = seed_from_live_as_git_repo(&run, &live, None).unwrap();
    // 非 repo → 返回 NotGitRepo
    assert!(matches!(result, GitSeedState::NotGitRepo));

    // base/staging 有业务文件
    assert_eq!(
        fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(run.staging_root().join("sub/b.md")).unwrap(),
        "world"
    );
    // staging 不应是 git repo（没有 .git/）
    assert!(!run.staging_root().join(".git").exists());
}

/// live 是 Git repo：staging 拿到 .git/ 元数据，worktree 仍是 live 当前工作区。
#[test]
fn git_staging_repo_clones_git_metadata_only() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "committed").unwrap();

    // 在 live 里建 git repo 并提交
    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    // 模拟 live 工作区有未提交修改
    fs::write(live.join("a.txt"), "working-dirty").unwrap();
    fs::write(live.join("untracked.md"), "untracked").unwrap();

    let run = StagingRun::create(tmp.path(), live.clone(), "git".to_string(), None).unwrap();
    let seed_state = seed_from_live_as_git_repo(&run, &live, None).unwrap();
    // repo → 返回 Existing
    assert!(matches!(seed_state, GitSeedState::Existing { .. }));

    // staging worktree 是 live 当前工作区（含未提交修改），不是 HEAD 的 checkout
    assert_eq!(
        fs::read_to_string(run.staging_root().join("a.txt")).unwrap(),
        "working-dirty"
    );
    assert_eq!(
        fs::read_to_string(run.staging_root().join("untracked.md")).unwrap(),
        "untracked"
    );
    // staging 拿到了 .git/ 元数据
    assert!(run.staging_root().join(".git").exists());
    // 临时目录已清理
    assert!(!run.run_root().join("git-repo").exists());
}

/// live 是 unborn Git repo：seed_from_live_as_git_repo 返回 Unborn。
#[test]
fn git_staging_unborn_repo_returns_unborn() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "not committed yet").unwrap();

    // 在 live 里建 git repo 但不提交（unborn）
    let _repo = git2::Repository::init(&live).unwrap();

    let run = StagingRun::create(tmp.path(), live.clone(), "git".to_string(), None).unwrap();
    let seed_state = seed_from_live_as_git_repo(&run, &live, None).unwrap();
    // unborn → 返回 Unborn
    assert!(matches!(seed_state, GitSeedState::Unborn { .. }));

    // staging 拿到了 .git/ 元数据
    assert!(run.staging_root().join(".git").exists());
}
