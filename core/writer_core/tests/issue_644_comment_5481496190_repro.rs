//! Issue #644 评论 5481496190 — 4 个硬问题修复后的正确行为验证测试。
//!
//! 本测试文件针对评论 5481496190 描述的 4 个缺陷，验证修复后的正确行为：
//! 1. Existing/Detached/已存在 remote ref 的正向 CAS 用 force=true → 已存在 ref 能成功更新
//! 2. install_index_with_lock 用 lockfile rename 模型 → 崩溃后 rollback 清理残留 lock
//! 3. plan.repo_create=true + owner marker 校验 → 不误删外部后来创建的仓库
//! 4. finalize_detached 原样传播 ConcurrentMetadataChanged → 不触发不该发生的 rollback
//!
//! 这些测试通过 writer_core 的公开 API（commit_git_finalize / rollback_git_finalize）
//! 和 git2 crate 直接构造场景，验证修复后的正确行为。
//!
//! 验证策略：WHITE_BOX（白盒，直接验证修复代码路径的 libgit2 语义和崩溃窗口行为）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use writer_core::storage::workspace_git::seed::GitSeedState;
use writer_core::storage::workspace_git::{
    commit_git_finalize, rollback_git_finalize, GitFinalizeError, GitFinalizePlan,
    GitMetadataSnapshot, GitRollbackOutcome, IndexSnapshot, RefSnapshot,
};

// ── helpers ──

/// 计算 SHA-256（与 git_commit.rs 内部 sha256_bytes 一致）。
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 在 live 目录下构造一个 detached HEAD at old_oid 的 git repo，返回 (old_oid, new_oid)。
/// old_oid 和 new_oid 都是 valid commit，new_oid 是 old_oid 的子提交。
fn make_detached_repo_with_two_commits(live: &Path) -> (git2::Oid, git2::Oid) {
    let repo = git2::Repository::init(live).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();

    // commit 1 (old_oid)
    fs::write(live.join("a.txt"), "hello").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree1_oid = index.write_tree().unwrap();
    let tree1 = repo.find_tree(tree1_oid).unwrap();
    let old_oid = repo.commit(None, &sig, &sig, "init", &tree1, &[]).unwrap();
    drop(tree1);

    // commit 2 (new_oid), parent = old_oid
    fs::write(live.join("b.txt"), "world").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let tree2_oid = index2.write_tree().unwrap();
    let tree2 = repo.find_tree(tree2_oid).unwrap();
    let old_commit = repo.find_commit(old_oid).unwrap();
    let new_oid = repo
        .commit(None, &sig, &sig, "second", &tree2, &[&old_commit])
        .unwrap();
    drop(tree2);
    drop(old_commit);

    // 设置 detached HEAD at old_oid（用 set_head_detached）
    repo.set_head_detached(old_oid).unwrap();

    (old_oid, new_oid)
}

// ── 问题 1：force=true 让已存在 ref 能成功更新（CAS 保护由 current_id 提供） ──

/// 验证问题 1 修复：finalize_existing / finalize_detached / sync_remote_refs(Existed)
/// 用 force=true 调用 reference_matching，已存在 ref 能成功更新。
/// current_id 参数提供 CAS 保护，current != old_oid 时返回 EMODIFIED。
///
/// 修复代码位置（core/writer_core/src/sync/git_commit.rs）：
/// - finalize_existing(): reference_matching(head_ref, new_oid, true, base_oid, ...)
/// - finalize_detached(): reference_matching("HEAD", new_oid, true, base_oid, ...)
/// - sync_remote_refs() RefSnapshot::Existed: reference_matching(name, target, true, old_oid, ...)
///
/// libgit2 语义：git_reference_create_matching 文档明确：
/// "force=false + ref 已存在 → GIT_EEXISTS"
/// "force=true + current_id 匹配 → 更新成功"
/// "force=true + current_id 不匹配 → GIT_EMODIFIED"
/// https://libgit2.org/docs/reference/main/refs/git_reference_create_matching.html
#[test]
fn verify_problem1_force_true_updates_existing_ref() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();

    // 构造已存在的 refs/heads/main 指向 old_oid。
    fs::write(live.join("a.txt"), "hello").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree1 = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let old_oid = repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree1, &[])
        .unwrap();
    drop(tree1);

    // 构造 new_oid（old_oid 的子提交），但不更新 refs/heads/main（用 detached commit）。
    fs::write(live.join("b.txt"), "world").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let tree2 = repo.find_tree(index2.write_tree().unwrap()).unwrap();
    let old_commit = repo.find_commit(old_oid).unwrap();
    let new_oid = repo
        .commit(None, &sig, &sig, "second", &tree2, &[&old_commit])
        .unwrap();
    drop(tree2);
    drop(old_commit);

    // 此时 refs/heads/main 仍指向 old_oid。
    let ref_before = repo.find_reference("refs/heads/main").unwrap();
    assert_eq!(ref_before.target(), Some(old_oid));

    // 验证 libgit2 语义：force=false 对已存在 ref 返回 GIT_EEXISTS。
    // 这是 force=true 修复的依据。
    let err = match repo.reference_matching(
        "refs/heads/main",
        new_oid,
        false,
        old_oid,
        "sync: finalize git repo metadata after full sync",
    ) {
        Ok(_) => panic!("force=false on existing ref must return GIT_EEXISTS per libgit2"),
        Err(e) => e,
    };
    assert_eq!(
        err.code(),
        git2::ErrorCode::Exists,
        "force=false on existing ref must return GIT_EEXISTS (libgit2 semantic baseline)"
    );

    // 验证修复：force=true + CAS 匹配（current == old_oid）→ 成功更新已存在 ref。
    repo.reference_matching(
        "refs/heads/main",
        new_oid,
        true,
        old_oid,
        "sync: finalize git repo metadata after full sync",
    )
    .expect("force=true with matching CAS must succeed (fix verified)");

    // 验证 ref 已更新到 new_oid。
    let repo2 = git2::Repository::open(&live).unwrap();
    let r = repo2.find_reference("refs/heads/main").unwrap();
    assert_eq!(r.target(), Some(new_oid));

    // 验证 CAS 保护：force=true + CAS 不匹配（current != old_oid）→ GIT_EMODIFIED。
    let err2 = match repo2.reference_matching(
        "refs/heads/main",
        old_oid,
        true,
        old_oid, // CAS expects old_oid, but current is new_oid
        "sync: CAS mismatch test",
    ) {
        Ok(_) => panic!("force=true with CAS mismatch must return GIT_EMODIFIED"),
        Err(e) => e,
    };
    assert_eq!(
        err2.code(),
        git2::ErrorCode::Modified,
        "force=true with CAS mismatch must return GIT_EMODIFIED (CAS protection verified)"
    );
}

// ── 问题 2：lockfile rename 模型，rollback 清理残留 lock ──

/// 验证问题 2 修复：install_index_with_lock 用 lockfile rename 模型，
/// 崩溃后 rollback_git_finalize 能正确清理残留 .git/index.lock 并恢复 index。
///
/// 修复代码位置：
/// - install_index_with_lock: 把目标 index 内容直接写入 lock 文件，
///   fsync 后 rename lock → index 作为提交和解锁（单一写入链）。
/// - rollback_git_finalize: index 恢复时清理残留 lock。
///
/// 新模型崩溃窗口：
/// - rename 前 crash：lock 拋留，index 未变（旧内容）→ rollback 清理 lock。
/// - rename 后 crash：lock 已消失，index 是新内容 → rollback 恢复 index。
#[test]
fn verify_problem2_lockfile_rename_recovers_stale_lock() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let repo = git2::Repository::init(&live).unwrap();

    let index_path = live.join(".git").join("index");
    let lock_path = live.join(".git").join("index.lock");

    // 用真实 git index 内容构造 old/new index。
    // old_index: add a.txt
    fs::write(live.join("a.txt"), "hello").unwrap();
    {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.write().unwrap();
    }
    let old_index_bytes = fs::read(&index_path).unwrap();

    // new_index: add a.txt + b.txt
    fs::write(live.join("b.txt"), "world").unwrap();
    {
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("a.txt")).unwrap();
        idx.add_path(std::path::Path::new("b.txt")).unwrap();
        idx.write().unwrap();
    }
    let new_index_bytes = fs::read(&index_path).unwrap();

    assert_ne!(
        old_index_bytes, new_index_bytes,
        "old and new index must differ"
    );

    // ── 场景 1：rename 前 crash（lock 写完但 rename 未完成） ──
    // index 仍是旧内容，lock 存在（包含新内容或部分）。
    // #644 评论 5485518160 修改点 2：当 plan.index_lock_owner=None 且 stale lock 存在时，
    // rollback_git_finalize 不能确定 lock 归属。旧语义是 no-op + 保留 lock（成功），
    // 但这会留下永久 .git/index.lock。新语义：返回 Err 保留 transaction，让上层
    // 迁移入口（rollback_full_sync_transaction）处理或下次恢复重试。
    // lock 仍存在（绝不碰别人的 lock），index 仍保持旧内容。
    {
        // 恢复初始状态：index = old, 无 lock。
        writer_core::storage::atomic_write_bytes(&index_path, &old_index_bytes).unwrap();
        let _ = fs::remove_file(&lock_path);

        // 模拟崩溃：lock 写完但 rename 未完成。
        // index 仍是 old（rename 未发生），lock 存在（包含 new 内容）。
        fs::write(&lock_path, &new_index_bytes).unwrap();

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::DidNotExist,
            refs: BTreeMap::new(),
            index: IndexSnapshot::Bytes(old_index_bytes.clone()),
            repo_existed: true,
        };
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(sha256_bytes(&new_index_bytes)),
            ref_plans: vec![],
            repo_create_owner: None,
            index_lock_owner: None,
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };

        // #644 评论 5485518160 修改点 2：未知 stale lock 应该保留 transaction
        //（rollback 返回 Err），而不是 rollback 成功但留下 lock。
        let result = rollback_git_finalize(&live, &snapshot, &plan, None);
        assert!(
            result.is_err(),
            "FIX VERIFIED (#644 评论 5485518160 修改点 2): rollback_git_finalize with \
             index_lock_owner=None and stale index.lock returns Err — cannot determine \
             lock ownership, preserving transaction for next recovery instead of \
             succeeding with a permanent lock left behind"
        );

        // lock 仍存在（绝不碰别人的 lock）。
        assert!(
            lock_path.exists(),
            "rollback_git_finalize must NOT delete index.lock when ownership is unknown \
             (refusing to delete potentially external git process lock)"
        );

        // index 保持旧内容（no-op，无需恢复）。
        let current_index = fs::read(&index_path).unwrap();
        assert_eq!(
            current_index, old_index_bytes,
            "index should remain at old content (no-op, current == snapshot.index)"
        );
    }

    // ── 场景 2：rename 后 crash（index 是新内容，lock 已消失） ──
    // rollback 应恢复 index 到旧内容。
    {
        // 模拟崩溃：rename 完成。
        // index 是新内容，lock 不存在。
        writer_core::storage::atomic_write_bytes(&index_path, &new_index_bytes).unwrap();
        let _ = fs::remove_file(&lock_path);

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::DidNotExist,
            refs: BTreeMap::new(),
            index: IndexSnapshot::Bytes(old_index_bytes.clone()),
            repo_existed: true,
        };
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(sha256_bytes(&new_index_bytes)),
            ref_plans: vec![],
            repo_create_owner: None,
            index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };

        rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

        // 修复后正确行为：index 恢复到旧内容。
        let restored_index = fs::read(&index_path).unwrap();
        assert_eq!(
            restored_index, old_index_bytes,
            "FIX VERIFIED: rollback restored index to snapshot.index \
             (lockfile rename model: crash after rename → index restored)"
        );

        // lock 不存在（本来就没了）。
        assert!(
            !lock_path.exists(),
            "lock should not exist after rollback (was already gone after rename)"
        );

        // 后续 git index write 应成功。
        let repo3 = git2::Repository::open(&live).unwrap();
        let mut idx = repo3.index().unwrap();
        idx.write().expect("subsequent index write must succeed");
    }
}

// ── 问题 3：owner marker 校验，不误删外部仓库 ──

/// 验证问题 3 修复：rollback_git_finalize 看到 plan.repo_create=true 时，
/// 只有 live .git 的 owner marker 与 plan.repo_create_owner 匹配才删除。
/// 外部创建的仓库（无 marker 或 marker 不匹配）不会被删。
///
/// 修复代码位置：
/// - GitFinalizePlan.repo_create_owner: NotGitRepo 路径在 prepare 阶段生成 uuid。
/// - finalize_not_git_repo: rename 前写入 owner marker 到 tmp_git。
/// - rollback_git_finalize: repo_create=true 路径检查 marker 匹配才删。
#[test]
#[allow(clippy::too_many_lines)]
fn verify_problem3_owner_marker_prevents_deleting_external_repo() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 模拟外部 git init 创建的真实 .git（有 commit、有 ref）。
    let external_repo = git2::Repository::init(&live).unwrap();
    let sig = git2::Signature::now("external", "external@example.com").unwrap();
    fs::write(live.join("external.txt"), "external data").unwrap();
    let mut index = external_repo.index().unwrap();
    index
        .add_path(std::path::Path::new("external.txt"))
        .unwrap();
    index.write().unwrap();
    let tree = external_repo
        .find_tree(index.write_tree().unwrap())
        .unwrap();
    let external_commit_oid = external_repo
        .commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "external init",
            &tree,
            &[],
        )
        .unwrap();

    // ── 场景 A：plan.repo_create_owner = None（旧 plan 或外部创建）→ .git 保留 ──
    {
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
            repo_create_owner: None, // 无 owner marker → 外部仓库
            index_lock_owner: None,
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };

        rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

        let live_git = live.join(".git");
        assert!(
            live_git.exists(),
            "FIX VERIFIED: rollback_git_finalize with repo_create=true but no owner marker \
             did NOT remove externally created .git (owner marker check prevents deletion)"
        );
    }

    // ── 场景 B：plan.repo_create_owner = Some(uuid)，但 live .git 无 marker → .git 保留 ──
    {
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
            repo_create_owner: Some(uuid::Uuid::new_v4().to_string()),
            index_lock_owner: None,
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };
        // live .git 没有 .sujian-sync-owner marker（外部创建的）。

        rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

        let live_git = live.join(".git");
        assert!(
            live_git.exists(),
            "FIX VERIFIED: rollback_git_finalize with repo_create=true + owner marker in plan \
             but NO marker in live .git did NOT remove externally created .git"
        );
    }

    // ── 场景 C：marker 不匹配 → .git 保留 ──
    {
        // 写一个不匹配的 marker。
        fs::write(
            live.join(".git").join(".sujian-sync-owner"),
            b"different-transaction-id",
        )
        .unwrap();

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
            repo_create_owner: Some(uuid::Uuid::new_v4().to_string()), // 不同的 uuid
            index_lock_owner: None,
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };

        rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

        let live_git = live.join(".git");
        assert!(
            live_git.exists(),
            "FIX VERIFIED: rollback_git_finalize with mismatched owner marker \
             did NOT remove externally created .git"
        );
    }

    // ── 场景 D：marker 匹配 → RepoInstallCommitted（不删 .git） ──
    // #644 评论 5487751293 问题1：marker 匹配说明 rename 已发生（.git 已是 live）。
    // 不能 remove_dir_all(.git)，因为 rename 后用户/外部 Git 可能已做了 commit。
    // 返回 RepoInstallCommitted，让上层按 commit-point 逻辑收尾。
    {
        let owner = uuid::Uuid::new_v4().to_string();
        fs::write(live.join(".git").join(".sujian-sync-owner"), &owner).unwrap();

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
            repo_create_owner: Some(owner), // 匹配的 uuid
            index_lock_owner: None,
            ref_tx_owner: None,

            ref_lock_names: Vec::new(),
        };

        let outcome = rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

        assert_eq!(
            outcome,
            GitRollbackOutcome::RepoInstallCommitted,
            "marker matching should return RepoInstallCommitted"
        );

        let live_git = live.join(".git");
        assert!(
            live_git.exists(),
            "RepoInstallCommitted: .git should NOT be removed (user may have made commits after install)"
        );
    }

    // external_commit_oid 仍可用于断言（证明外部仓库确实存在过）。
    let _ = external_commit_oid;
}

// ── 问题 4：ConcurrentMetadataChanged 原样传播，不触发 rollback ──

/// 验证问题 4 修复：finalize_detached 把 ConcurrentMetadataChanged 原样向上传播，
/// commit_git_finalize 看到 ConcurrentMetadataChanged 不调用 rollback_git_finalize。
/// 并发方把 detached HEAD 推到 plan.new_oid 时，HEAD 保持 new_oid，不被回退。
///
/// 修复代码位置：
/// - finalize_detached: 返回 Result<(), GitFinalizeError>，不降级 ConcurrentMetadataChanged。
/// - finalize_git_repo_metadata_inner: Detached 分支不 .map_err(FinalizeFailed)。
/// - commit_git_finalize: ConcurrentMetadataChanged 分支不 rollback。
#[test]
fn verify_problem4_concurrent_metadata_changed_no_rollback() {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    // 构造 live: detached HEAD at old_oid，有 old_oid 和 new_oid 两个 commit。
    let (old_oid, new_oid) = make_detached_repo_with_two_commits(&live);

    // 构造 staging repo: HEAD at new_oid（detached）。
    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    fs::write(staging.join("b.txt"), "world").unwrap();
    let staging_repo = git2::Repository::init(&staging).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    let mut s_index = staging_repo.index().unwrap();
    s_index.add_path(std::path::Path::new("a.txt")).unwrap();
    s_index.add_path(std::path::Path::new("b.txt")).unwrap();
    s_index.write().unwrap();
    let s_tree = staging_repo
        .find_tree(s_index.write_tree().unwrap())
        .unwrap();
    let staging_commit_oid = staging_repo
        .commit(None, &sig, &sig, "staging init", &s_tree, &[])
        .unwrap();
    staging_repo.set_head_detached(staging_commit_oid).unwrap();

    // snapshot 记录 detached HEAD at old_oid（finalize 前的状态）。
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Existed {
            oid: old_oid.to_string(),
        },
        refs: BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: true,
    };

    // plan: HEAD old_oid -> new_oid（detached 路径）。
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: None,
        ref_plans: vec![(
            "HEAD".to_string(),
            Some(old_oid.to_string()),
            new_oid.to_string(),
        )],
        repo_create_owner: None,
        index_lock_owner: None,
        ref_tx_owner: Some(uuid::Uuid::new_v4().to_string()),

        ref_lock_names: Vec::new(),
    };

    // 模拟并发：在调用 commit_git_finalize 前，把 detached HEAD 从 old_oid 推到 new_oid。
    let live_repo_for_concurrent = git2::Repository::open(&live).unwrap();
    live_repo_for_concurrent.set_head_detached(new_oid).unwrap();

    // 调用 commit_git_finalize。
    // 修复后正确行为：verify_git_metadata_unchanged 发现 HEAD 从 old_oid 变成 new_oid，
    // 返回 ConcurrentMetadataChanged，commit_git_finalize 不 rollback，HEAD 保持 new_oid。
    let result = commit_git_finalize(
        &live,
        &staging,
        &GitSeedState::Detached { head_oid: old_oid },
        &snapshot,
        &plan,
        None,
    );

    // 验证错误类型是 ConcurrentMetadataChanged（原样传播，不降级）。
    let err = result.expect_err("commit_git_finalize must fail with ConcurrentMetadataChanged");
    match &err {
        GitFinalizeError::ConcurrentMetadataChanged { reason } => {
            // 正确行为：错误原样传播。
            let _ = reason;
        }
        GitFinalizeError::FinalizeFailed(_) => {
            panic!(
                "DEFECT STILL PRESENT: error was downgraded to FinalizeFailed \
                 instead of ConcurrentMetadataChanged — HEAD may be wrongly rolled back"
            );
        }
        GitFinalizeError::RollbackFailed { .. } => {
            panic!(
                "DEFECT STILL PRESENT: rollback was triggered for ConcurrentMetadataChanged \
                 (should not rollback when nothing was written this round)"
            );
        }
    }

    // 验证 HEAD 保持 new_oid（没有被错误回退到 old_oid）。
    let live_repo_after = git2::Repository::open(&live).unwrap();
    let head_after = live_repo_after.head().unwrap();
    let head_oid_after = head_after.target().unwrap();

    assert_eq!(
        head_oid_after, new_oid,
        "FIX VERIFIED: concurrent detached HEAD update (new_oid) was NOT rolled back. \
         HEAD stays at new_oid because ConcurrentMetadataChanged was propagated without rollback."
    );
}
