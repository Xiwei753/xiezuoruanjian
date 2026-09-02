//! Issue #644 评论 5496728184 — 对抗式验证：覆盖 Prepared 分支三个 Err 路径。
//!
//! 复现测试已覆盖 (false, true, Missing) → 补推进 和 Completed 删除失败 → Err。
//! 本测试覆盖剩余三个 Err 分支，确认修复后不猜 ownership、不吞 Corrupt：
//! - (true, true, Missing) → ambiguous ownership → Err
//! - (false, false, Missing) → both missing → Err
//! - (false, false, Corrupt) → target corrupt → Err
//!
//! 这是验证 agent 构造的对抗式探针，不属于原始复现测试。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

fn canonicalize_or_lossy(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// 构造 journal 文件的辅助函数。
fn write_journal(
    worktree_root: &std::path::Path,
    git_dir: &std::path::Path,
    original_source: &std::path::Path,
    claimed_source: &std::path::Path,
    owner: &str,
) -> PathBuf {
    let target_tmp = git_dir
        .parent()
        .unwrap()
        .join(format!(".git.sujian-migrate-tmp-{}", owner));
    let migrations_dir = git_dir.parent().unwrap().join(".layout-migrations");
    fs::create_dir_all(&migrations_dir).unwrap();
    let journal_path = migrations_dir.join(format!("{}.json", owner));

    let worktree_canonical = canonicalize_or_lossy(worktree_root);
    let journal_json = serde_json::json!({
        "owner": owner,
        "worktree_canonical": worktree_canonical,
        "original_source": original_source.to_string_lossy(),
        "claimed_source": claimed_source.to_string_lossy(),
        "target_tmp": target_tmp.to_string_lossy(),
        "target_git_dir": git_dir.to_string_lossy(),
        "phase": "prepared"
    });
    fs::write(&journal_path, serde_json::to_string(&journal_json).unwrap()).unwrap();
    journal_path
}

/// (true, true, Missing) → original 和 claimed 同时存在，ownership 不明确 → Err
#[test]
fn adversarial_prepared_true_true_missing_returns_err() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let worktree_root = root.join("worktree");
    let git_dir = root.join("private").join("project.git");
    fs::create_dir_all(&worktree_root).unwrap();
    fs::create_dir_all(git_dir.parent().unwrap()).unwrap();

    let owner = "adv-owner-ttm";
    let original_source = worktree_root.join(".git");
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));

    // 构造 (true, true, Missing)：original 和 claimed 都存在
    // original_source: 一个有效 git 仓库
    let repo = git2::Repository::init(&worktree_root).unwrap();
    drop(repo);
    assert!(original_source.exists(), "setup: original 应存在");

    // claimed_source: 也创建一个目录（模拟 rename 目标已存在但 original 未删的异常状态）
    fs::create_dir_all(&claimed_source).unwrap();
    // 在 claimed 里放一个文件让它非空（更像真实 claimed 仓库）
    fs::write(claimed_source.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    assert!(claimed_source.exists(), "setup: claimed 应存在");
    assert!(!git_dir.exists(), "setup: target 应不存在 (Missing)");

    let journal_path = write_journal(
        &worktree_root,
        &git_dir,
        &original_source,
        &claimed_source,
        owner,
    );
    assert!(journal_path.exists());

    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        worktree_root.clone(),
        git_dir.clone(),
    );
    let result = writer_core::storage::git_repo_layout::ensure_project_repo_with_layout(&layout);

    eprintln!(
        "[ADV-TTM] (true,true,Missing) result: {:?}",
        result.as_ref().err()
    );

    // 修复后应返回 Err（ambiguous ownership），不能猜一个继续
    assert!(
        result.is_err(),
        "对抗式 (true,true,Missing): 应返回 Err — original 和 claimed 同时存在时 \
         ownership 不明确，不能猜一个继续。实际: {:?}",
        result
    );
    // journal 不应被删（Err 后保留供人工介入）
    assert!(
        journal_path.exists(),
        "对抗式 (true,true,Missing): journal 应保留 — Err 后不能删恢复记录"
    );
    eprintln!("[ADV-TTM] 验证通过: (true,true,Missing) 正确返回 Err");
}

/// (false, false, Missing) → original 和 claimed 都不存在 → Err（不能清 journal）
#[test]
fn adversarial_prepared_false_false_missing_returns_err() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let worktree_root = root.join("worktree");
    let git_dir = root.join("private").join("project.git");
    fs::create_dir_all(&worktree_root).unwrap();
    fs::create_dir_all(git_dir.parent().unwrap()).unwrap();

    let owner = "adv-owner-ffm";
    let original_source = worktree_root.join(".git");
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));

    // 构造 (false, false, Missing)：original 和 claimed 都不存在
    // 不 init 仓库，不创建 claimed
    assert!(!original_source.exists(), "setup: original 应不存在");
    assert!(!claimed_source.exists(), "setup: claimed 应不存在");
    assert!(!git_dir.exists(), "setup: target 应不存在 (Missing)");

    let journal_path = write_journal(
        &worktree_root,
        &git_dir,
        &original_source,
        &claimed_source,
        owner,
    );
    assert!(journal_path.exists());

    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        worktree_root.clone(),
        git_dir.clone(),
    );
    let result = writer_core::storage::git_repo_layout::ensure_project_repo_with_layout(&layout);

    eprintln!(
        "[ADV-FFM] (false,false,Missing) result: {:?}",
        result.as_ref().err()
    );

    // 修复后应返回 Err（两边都 missing，不能清 journal 假装完成）
    assert!(
        result.is_err(),
        "对抗式 (false,false,Missing): 应返回 Err — original 和 claimed 都不存在时 \
         不能清 journal 假装完成。实际: {:?}",
        result
    );
    assert!(
        journal_path.exists(),
        "对抗式 (false,false,Missing): journal 应保留 — 不能清恢复记录"
    );
    eprintln!("[ADV-FFM] 验证通过: (false,false,Missing) 正确返回 Err");
}

/// (_, _, Corrupt) → target 路径存在但仓库损坏 → Err（不吞 Corrupt）
#[test]
fn adversarial_prepared_corrupt_target_returns_err() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let worktree_root = root.join("worktree");
    let git_dir = root.join("private").join("project.git");
    fs::create_dir_all(&worktree_root).unwrap();
    fs::create_dir_all(git_dir.parent().unwrap()).unwrap();

    let owner = "adv-owner-corr";
    let original_source = worktree_root.join(".git");
    let claimed_source = worktree_root.join(format!(".git.sujian-migrate-source-{}", owner));

    // 构造 (false, false, Corrupt)：target 存在但不是有效 git 仓库
    // 创建 target 目录但放垃圾内容（不是有效 git 仓库）
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("not_a_git_repo"), "garbage").unwrap();
    // 确认 target 是 Corrupt：路径存在但 git2::Repository::open 失败
    assert!(git_dir.exists(), "setup: target 应存在");
    assert!(
        git2::Repository::open(&git_dir).is_err(),
        "setup: target 应是 Corrupt（非有效 git 仓库）"
    );

    assert!(!original_source.exists(), "setup: original 应不存在");
    assert!(!claimed_source.exists(), "setup: claimed 应不存在");

    let journal_path = write_journal(
        &worktree_root,
        &git_dir,
        &original_source,
        &claimed_source,
        owner,
    );
    assert!(journal_path.exists());

    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
        worktree_root.clone(),
        git_dir.clone(),
    );
    let result = writer_core::storage::git_repo_layout::ensure_project_repo_with_layout(&layout);

    eprintln!(
        "[ADV-CORR] (_,_,Corrupt) result: {:?}",
        result.as_ref().err()
    );

    // 修复后应返回 Err（target corrupt，不能吞成普通状态）
    assert!(
        result.is_err(),
        "对抗式 (_,_,Corrupt): 应返回 Err — target 路径存在但仓库损坏时 \
         不能吞成普通状态继续。实际: {:?}",
        result
    );
    assert!(
        journal_path.exists(),
        "对抗式 (_,_,Corrupt): journal 应保留 — Corrupt 后不能删恢复记录"
    );
    eprintln!("[ADV-CORR] 验证通过: (_,_,Corrupt) 正确返回 Err");
}
