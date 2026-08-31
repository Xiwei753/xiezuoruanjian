//! Issue #644 评论 5482310913 — 3 个跨资源一致性 bug 的复现测试。
//!
//! 本测试文件针对评论 5482310913 描述的 3 个会破坏跨资源一致性的缺陷：
//! 1. NotGitRepo owner marker 删除得太早：`finalize_not_git_repo` rename 成功后立即
//!    删 marker，但上层 `sync_ops.rs` 的 `tx.finish()` 在之后才执行 → 崩溃窗口
//!    "marker 已删 + tx.finish() 未执行" → 重启恢复时 rollback 当作外部仓库保留
//!    → "旧文件 + 新 Git metadata"。
//! 2. `rollback_git_finalize` 在确认 repo_create ownership 之前先碰 index：先处理
//!    `.git/index` / `.git/index.lock`（line 766），后才检查 ownership（line 818）。
//!    外部 git init 创建的 .git 会被误碰 index/lock。
//! 3. Git rollback 的 CAS miss 只是 warn + Ok(())：`rollback_full_sync_transaction`
//!    看到 Ok 后会继续恢复正文 backup 并删除 transaction → "新并发 Git metadata
//!    保留 + 工作区文件回滚成旧内容"。
//!
//! 验证策略：WHITE_BOX（白盒，直接验证修复代码路径的崩溃窗口行为）。
//! 这些测试在修复前 FAIL（体现 bug 存在），修复后 PASS。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use writer_core::sync::git_commit::{
    commit_git_finalize, rollback_git_finalize, GitFinalizeError, GitFinalizePlan,
    GitMetadataSnapshot, IndexSnapshot, RefSnapshot,
};
use writer_core::sync::git_staging::GitSeedState;

// ── helpers ──

/// 计算 SHA-256（与 git_commit.rs 内部 sha256_bytes 一致）。
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 构造一个有 commit 的 staging git repo，HEAD 指向 refs/heads/main。
/// 返回 commit oid。用于 NotGitRepo 路径的 finalize 测试。
fn make_staging_repo_with_commit(staging: &Path) -> git2::Oid {
    fs::create_dir_all(staging).unwrap();
    let repo = git2::Repository::init(staging).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let oid = repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    // 确保 HEAD 指向 refs/heads/main（非 detached）。
    repo.set_head("refs/heads/main").unwrap();
    oid
}

// ── 问题 1：NotGitRepo owner marker 删除得太早 ──

/// 验证问题 1：`finalize_not_git_repo` rename 成功后 owner marker 必须仍然存在，
/// 直到上层 `tx.finish()` 之后才允许清理。
///
/// 当前 bug（core/writer_core/src/sync/git_commit.rs line 1231-1234）：
/// `finalize_not_git_repo` 在 `fs::rename(tmp_git, live_git)` 成功后立即删除
/// `.git/.sujian-sync-owner`。但上层 `sync_ops.rs` line 825 的 `tx.finish()`
/// 在 `try_commit_git_finalize` 返回 Ok 之后才执行。
///
/// 崩溃窗口：marker 已删 + tx.finish() 未执行
/// → 重启时 manifest 仍是 `FilesCommittedPendingGit` 待恢复事务，
///   但 live .git 是新仓库且无 owner marker
/// → `rollback_git_finalize` 的 ownership 判定（line 818-837）当作外部仓库保留 .git
/// → `rollback_full_sync_transaction` 继续回滚正文文件
/// → "旧文件 + 新 Git metadata" 的跨资源不一致状态。
///
/// 修复方向：marker 删除应推迟到 `tx.finish()` 之后（或由 finish 负责），
/// `commit_git_finalize` 成功返回时 marker 必须仍在 live .git 中。
///
/// 修复前：`commit_git_finalize` 成功后 marker 不存在 → 测试 FAIL。
/// 修复后：`commit_git_finalize` 成功后 marker 仍存在 → 测试 PASS。
#[test]
fn verify_problem1_marker_must_survive_finalize_until_tx_finish() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let staging = tmp.path().join("staging");

    let new_oid = make_staging_repo_with_commit(&staging);

    let owner = uuid::Uuid::new_v4().to_string();
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: false,
    };
    let plan = GitFinalizePlan {
        repo_create: true,
        new_index_sha256: None,
        ref_plans: vec![],
        repo_create_owner: Some(owner.clone()),
        index_lock_owner: None,
    };

    // 调用 commit_git_finalize（NotGitRepo 路径）。
    // 成功后 live/.git 应已安装，但 marker 必须保留到 tx.finish()。
    commit_git_finalize(&live, &staging, &GitSeedState::NotGitRepo, &snapshot, &plan)
        .expect("commit_git_finalize NotGitRepo must succeed (baseline)");

    let live_git = live.join(".git");
    assert!(
        live_git.exists(),
        "baseline: live/.git must exist after successful NotGitRepo finalize"
    );

    let marker_path = live_git.join(".sujian-sync-owner");
    assert!(
        marker_path.exists(),
        "FIX REQUIRED: owner marker must survive commit_git_finalize until tx.finish(). \
         Current bug: finalize_not_git_repo deletes marker at line 1231-1234 immediately \
         after rename, but sync_ops.rs tx.finish() is called later at line 825. \
         Crash window (marker deleted + tx.finish() not executed) leaves recovery \
         seeing no owner marker → rollback treats live .git as external → keeps new \
         Git metadata while rolling back workspace files → 'old files + new Git metadata'."
    );

    // marker 内容应仍是 owner uuid（未被篡改）。
    let marker_content = fs::read_to_string(&marker_path).unwrap();
    assert_eq!(
        marker_content, owner,
        "marker content must remain the owner uuid until tx.finish()"
    );

    let _ = new_oid;
}

// ── 问题 2：rollback ownership 判断顺序 ──

/// 验证问题 2：`rollback_git_finalize` 在 `plan.repo_create=true` 且 live .git 无匹配
/// owner marker 时，必须一字节不碰 `.git/index` / `.git/index.lock` / refs，直接返回。
///
/// 当前 bug（core/writer_core/src/sync/git_commit.rs）：
/// `rollback_git_finalize` 先处理 index（line 766-810），后才检查 repo_create
/// ownership（line 818-840）。
///
/// 触发路径：
/// - prepare 时 live 不是 Git repo（plan.repo_create=true）
/// - app 在 .git rename 前崩溃（manifest 已落盘，live/.git 未出现）
/// - 用户自己 `git init` 创建 live/.git（无本轮 owner marker）
/// - app 重启恢复，调用 rollback_git_finalize
///
/// 此时 .git 没有本轮 owner marker，本应一字节不碰，但当前代码会先检查/恢复 index，
/// 甚至删除 index.lock，最后才发现 marker 不匹配。
///
/// 修复方向：先判 ownership，不匹配直接返回 Ok(())，不碰 index/refs。
///
/// 修复前：index.lock 被删 + index 可能被改 → 测试 FAIL。
/// 修复后：index.lock 仍存在 + index 内容不变 → 测试 PASS。
#[test]
fn verify_problem2_rollback_must_check_ownership_before_touching_index() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 模拟外部 git init 创建的 .git（有 commit、有 index、无 owner marker）。
    let external_repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("external", "external@example.com").unwrap();
    fs::write(live.join("external.txt"), "external data").unwrap();
    {
        let mut index = external_repo.index().unwrap();
        index
            .add_path(std::path::Path::new("external.txt"))
            .unwrap();
        index.write().unwrap();
        let tree = external_repo
            .find_tree(index.write_tree().unwrap())
            .unwrap();
        let _ = external_repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "external init",
                &tree,
                &[],
            )
            .unwrap();
    }
    drop(external_repo);

    let index_path = live.join(".git").join("index");
    let lock_path = live.join(".git").join("index.lock");
    let original_index_bytes = fs::read(&index_path).unwrap();

    // 模拟外部进程留下的 index.lock（或之前崩溃的残留）。
    // rollback 不应删除它，因为这个 .git 不是本轮创建的。
    fs::write(&lock_path, b"stale-lock-content-from-external-process").unwrap();

    // plan: repo_create=true, owner marker = 某 uuid，但 live .git 无 marker（外部创建）。
    // new_index_sha256 设一个与当前 index 不同的值，使 index_is_ours=false，
    // 触发"index 不是新内容但 lock 存在"分支（line 793-803），当前代码会删除 lock。
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: false,
    };
    let plan = GitFinalizePlan {
        repo_create: true,
        new_index_sha256: Some(sha256_bytes(b"some-new-index-not-matching-current")),
        ref_plans: vec![],
        repo_create_owner: Some(uuid::Uuid::new_v4().to_string()),
        index_lock_owner: None,
    };

    rollback_git_finalize(&live, &snapshot, &plan)
        .expect("rollback_git_finalize must return Ok (ownership mismatch → no-op)");

    // 修复后正确行为：index.lock 不应被删（ownership 不匹配，应直接返回不碰任何东西）。
    assert!(
        lock_path.exists(),
        "FIX REQUIRED: rollback_git_finalize with repo_create=true but no matching owner \
         marker must NOT touch .git/index.lock. Current bug: index handling at line 766 \
         runs before ownership check at line 818 → external repo's index.lock gets deleted \
         at line 799, violating 'do not touch external repos' invariant."
    );

    // lock 内容不应改变。
    let lock_content = fs::read(&lock_path).unwrap();
    assert_eq!(
        lock_content, b"stale-lock-content-from-external-process",
        "FIX REQUIRED: rollback_git_finalize must not modify external repo's index.lock content"
    );

    // index 内容不应改变。
    let current_index = fs::read(&index_path).unwrap();
    assert_eq!(
        current_index, original_index_bytes,
        "FIX REQUIRED: rollback_git_finalize with no matching owner marker \
         must NOT modify .git/index (external repo must be left untouched)"
    );
}

// ── 问题 3：CAS miss 返回 Ok 导致文件误回滚 ──

/// 验证问题 3：`rollback_git_finalize` 在 ref CAS miss（当前值既不是 old_oid 也不是
/// new_oid，是真正的并发新状态）时必须返回明确错误（非 Ok），使上层
/// `rollback_full_sync_transaction` 不会继续回滚正文文件。
///
/// 当前 bug（core/writer_core/src/sync/git_commit.rs line 929-935）：
/// `rollback_git_finalize` 对 ref 的 CAS miss 是 `log::warn!` + 跳过 + 最后返回 `Ok(())`。
/// `storage/transaction.rs::rollback_full_sync_transaction`（line 467-471）看到 Git
/// rollback `Ok` 后会继续恢复正文/engine-state backup（line 474-508）并删除 transaction
/// → "新并发 Git metadata 保留 + 工作区文件回滚成旧内容" 的跨资源不一致。
///
/// 修复方向：CAS miss 时返回 `Err`（或明确的非 Ok 结果），让上层保留 transaction
/// 不回滚文件，给下次恢复留机会。
///
/// 修复前：返回 `Ok(())` → 测试 FAIL。
/// 修复后：返回 `Err` → 测试 PASS。
#[test]
fn verify_problem3_ref_cas_miss_must_return_err_not_ok() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 构造 live git repo，有三个 commit：old → new → concurrent。
    // 最终 refs/heads/main 指向 concurrent_oid（并发新状态）。
    let repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();

    // old commit
    fs::write(live.join("a.txt"), "old").unwrap();
    let old_oid = {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        repo.commit(Some("refs/heads/main"), &sig, &sig, "old", &tree, &[])
            .unwrap()
    };

    // new commit (parent = old) — 用 detached commit 避免 libgit2
    // "current tip is not the first parent" 检查（refs/heads/main 仍指向 old_oid）。
    fs::write(live.join("b.txt"), "new").unwrap();
    let new_oid = {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.add_path(std::path::Path::new("b.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parent = repo.find_commit(old_oid).unwrap();
        repo.commit(None, &sig, &sig, "new", &tree, &[&parent])
            .unwrap()
    };

    // concurrent commit (parent = new, 完全不同的 oid) — 同样用 detached commit。
    // 模拟并发进程在 finalize 后又推了一个新 commit。
    fs::write(live.join("c.txt"), "concurrent").unwrap();
    let concurrent_oid = {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.add_path(std::path::Path::new("b.txt")).unwrap();
        idx.add_path(std::path::Path::new("c.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parent = repo.find_commit(new_oid).unwrap();
        repo.commit(None, &sig, &sig, "concurrent", &tree, &[&parent])
            .unwrap()
    };

    // 把 refs/heads/main 强制指向 concurrent_oid（模拟并发进程推送）。
    repo.reference(
        "refs/heads/main",
        concurrent_oid,
        true,
        "concurrent: force push",
    )
    .unwrap();

    // 此时 refs/heads/main 指向 concurrent_oid。
    // plan.ref_plans 说本轮要把 refs/heads/main 从 old_oid 更新到 new_oid。
    // rollback 时 current=concurrent_oid：
    //   - current != new_oid → 不是我们写的（CAS miss）
    //   - current != old_oid → 不是未执行的状态
    //   → 真正的并发新状态，不能覆盖，也不能让上层回滚文件。
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Existed {
            oid: old_oid.to_string(),
        },
        refs: BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: None,
        ref_plans: vec![(
            "refs/heads/main".to_string(),
            Some(old_oid.to_string()),
            new_oid.to_string(),
        )],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = rollback_git_finalize(&live, &snapshot, &plan);

    // 修复后正确行为：CAS miss 必须返回 Err，使上层 rollback_full_sync_transaction
    // 不会继续恢复 backup 文件并删除 transaction。
    assert!(
        result.is_err(),
        "FIX REQUIRED: rollback_git_finalize ref CAS miss (current={} is neither \
         old={} nor new={}) must return Err, not Ok(()). Current bug: line 929-935 \
         does warn + skip + final Ok(()), then rollback_full_sync_transaction sees \
         Ok and continues restoring backup files + deleting transaction → \
         'new concurrent Git metadata kept + workspace files rolled back to old' \
         cross-resource inconsistency.",
        concurrent_oid,
        old_oid,
        new_oid
    );

    // 验证 refs/heads/main 仍是 concurrent_oid（未被错误覆盖）。
    // 即使返回 Err，ref 本身不应被改（CAS miss 跳过 ref 写入是对的，
    // 错的是最终返回 Ok 让上层继续回滚文件）。
    let repo_after = git2::Repository::open(&live).unwrap();
    let ref_after = repo_after.find_reference("refs/heads/main").unwrap();
    assert_eq!(
        ref_after.target(),
        Some(concurrent_oid),
        "concurrent ref must remain at concurrent_oid (CAS miss correctly skipped ref write)"
    );
}

// ── 问题 3 补充：index CAS miss 也必须返回 Err ──

/// 验证问题 3 的 index 维度：`rollback_git_finalize` 在 index CAS miss
/// （当前 index 既不等于 snapshot.index 也不等于 plan.new_index_sha256）时
/// 也必须返回明确错误（非 Ok）。
///
/// 当前 bug（line 804-809）：index CAS miss 是 `log::warn!` + 跳过 + 继续，
/// 最后返回 `Ok(())`。上层 `rollback_full_sync_transaction` 看到 Ok 后继续回滚文件。
///
/// 修复前：返回 `Ok(())` → 测试 FAIL。
/// 修复后：返回 `Err` → 测试 PASS。
#[test]
fn verify_problem3_index_cas_miss_must_return_err_not_ok() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 构造 live git repo，有 index。
    let repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();
    {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let _ = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    drop(repo);

    let index_path = live.join(".git").join("index");

    // 模拟并发进程改了 index（既不是 snapshot.index 也不是 plan.new_index_sha256）。
    // 写一个完全不同的 index 内容（不是有效 git index 也行，rollback 只比较 hash）。
    fs::write(&index_path, b"concurrent-index-content-from-other-process").unwrap();
    let concurrent_index_bytes = fs::read(&index_path).unwrap();

    // snapshot.index = 旧 index（与当前不同）。
    // plan.new_index_sha256 = "new-index" 的 hash（与当前不同）。
    // 当前 index 是 concurrent（与两者都不同）→ CAS miss。
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Bytes(b"old-index-snapshot".to_vec()),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(b"new-index-we-planned-to-write")),
        ref_plans: vec![],
        repo_create_owner: None,
        index_lock_owner: None,
    };

    let result = rollback_git_finalize(&live, &snapshot, &plan);

    assert!(
        result.is_err(),
        "FIX REQUIRED: rollback_git_finalize index CAS miss (current index hash matches \
         neither snapshot.index nor plan.new_index_sha256) must return Err, not Ok(()). \
         Current bug: line 804-809 does warn + skip + final Ok(()), then \
         rollback_full_sync_transaction sees Ok and continues restoring backup files \
         + deleting transaction → cross-resource inconsistency."
    );

    // index 内容不应被改（CAS miss 跳过 index 写入是对的，错的是返回 Ok）。
    let current_index = fs::read(&index_path).unwrap();
    assert_eq!(
        current_index, concurrent_index_bytes,
        "concurrent index must remain unchanged (CAS miss correctly skipped index write)"
    );
}

// ── 问题 1 补充：marker 删除时机不应在 commit_git_finalize 内部 ──

/// 验证问题 1 的不变量：`commit_git_finalize` 成功返回后，若 plan.repo_create_owner
/// 为 Some，则 live .git 中必须仍存在 owner marker 文件。
///
/// 这是上层 `tx.finish()` 能安全清理 marker 的前提。如果 `commit_git_finalize`
/// 内部就删了 marker，那么 "finalize 成功 + tx.finish() 未执行" 的崩溃窗口
/// 会让恢复流程丢失 ownership 证据。
///
/// 修复前：marker 被删 → 测试 FAIL。
/// 修复后：marker 保留 → 测试 PASS。
#[test]
fn verify_problem1_invariant_marker_present_after_successful_finalize() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let staging = tmp.path().join("staging");

    make_staging_repo_with_commit(&staging);

    let owner = uuid::Uuid::new_v4().to_string();
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::DidNotExist,
        refs: BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: false,
    };
    let plan = GitFinalizePlan {
        repo_create: true,
        new_index_sha256: None,
        ref_plans: vec![],
        repo_create_owner: Some(owner),
        index_lock_owner: None,
    };

    let result = commit_git_finalize(&live, &staging, &GitSeedState::NotGitRepo, &snapshot, &plan);

    // finalize 必须成功（baseline）。
    match &result {
        Ok(()) => {}
        Err(GitFinalizeError::FinalizeFailed(e)) => {
            panic!("commit_git_finalize must succeed (baseline): FinalizeFailed({e})");
        }
        Err(GitFinalizeError::ConcurrentMetadataChanged { reason }) => {
            panic!(
                "commit_git_finalize must succeed (baseline): ConcurrentMetadataChanged({reason})"
            );
        }
        Err(GitFinalizeError::RollbackFailed { finalize, rollback }) => {
            panic!(
                "commit_git_finalize must succeed (baseline): RollbackFailed({finalize}, {rollback})"
            );
        }
    }

    let marker_path = live.join(".git").join(".sujian-sync-owner");
    assert!(
        marker_path.exists(),
        "FIX REQUIRED: invariant — after successful commit_git_finalize with \
         repo_create_owner=Some, live .git MUST still contain owner marker. \
         Marker cleanup belongs to tx.finish() (after manifest is marked Finished), \
         not to commit_git_finalize (which returns before tx.finish())."
    );
}
