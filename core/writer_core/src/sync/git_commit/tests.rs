use super::*;
use tempfile::TempDir;

#[test]
fn prepare_snapshot_non_repo() {
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();

    let (snapshot, plan) =
        prepare_git_finalize(&live, &GitSeedState::NotGitRepo, &staging, None).unwrap();
    assert!(matches!(snapshot.head, RefSnapshot::DidNotExist));
    assert!(snapshot.refs.is_empty());
    assert!(matches!(snapshot.index, IndexSnapshot::Missing));
    assert!(!snapshot.repo_existed);
    // #644 评论 5480360027：NotGitRepo 路径 plan.repo_create == true。
    assert!(plan.repo_create);
    // staging 无 .git → 无目标 index hash。
    assert!(plan.new_index_sha256.is_none());
    assert!(plan.ref_plans.is_empty());
}

#[test]
fn prepare_snapshot_unborn_repo() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let _repo = git2::Repository::init(&live).unwrap();

    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();

    let seed = GitSeedState::Unborn {
        head_ref: "refs/heads/main".to_string(),
    };
    let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging, None).unwrap();
    assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
    assert!(snapshot.refs.contains_key("refs/heads/main"));
    assert!(snapshot.repo_existed);
    // #644 评论 5480360027：Unborn 路径 plan.repo_create == false。
    assert!(!plan.repo_create);
    // staging 无 .git → 无目标 index hash，无 ref_plans。
    assert!(plan.new_index_sha256.is_none());
    assert!(plan.ref_plans.is_empty());
}

#[test]
fn prepare_snapshot_existing_repo() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    let commit_oid = repo
        .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();

    let seed = GitSeedState::Existing {
        head_ref: "refs/heads/main".to_string(),
        head_oid: commit_oid,
    };
    let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging, None).unwrap();
    assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
    assert!(matches!(snapshot.index, IndexSnapshot::Bytes(_)));
    assert!(snapshot.refs.contains_key("refs/heads/main"));
    assert!(snapshot.repo_existed);
    // #644 评论 5480360027：Existing 路径 plan.repo_create == false。
    assert!(!plan.repo_create);
    // staging 无 .git → 无目标 index hash，无 ref_plans。
    assert!(plan.new_index_sha256.is_none());
    assert!(plan.ref_plans.is_empty());
}

#[test]
fn rollback_restores_index() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();

    let seed = GitSeedState::Existing {
        head_ref: "refs/heads/main".to_string(),
        head_oid: git2::Oid::zero(),
    };
    let (snapshot, _plan) = prepare_git_finalize(&live, &seed, &staging, None).unwrap();
    let original_index = match &snapshot.index {
        IndexSnapshot::Bytes(b) => b.clone(),
        _ => panic!("expected IndexSnapshot::Bytes"),
    };

    // 修改 index。
    fs::write(live.join("b.txt"), "new").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("b.txt")).unwrap();
    index.write().unwrap();

    // Rollback 应恢复原始 index。
    // #644 评论 5480360027：CAS-based rollback 使用 write-ahead plan。
    // #644 评论 5484539222 缺陷1：rollback 反向恢复路径需要 index_lock_owner 做 OwnedIndexLock。
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(sha256_bytes(
            &fs::read(live.join(".git").join("index")).unwrap(),
        )),
        ref_plans: Vec::new(),
        repo_create_owner: None,
        index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
        ref_tx_owner: None,
        ref_lock_names: Vec::new(),
    };
    rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();
    let restored_index = fs::read(live.join(".git").join("index")).unwrap();
    assert_eq!(restored_index, original_index);
}

/// #644 评论 5480360027：验证 write-ahead plan 在 prepare 阶段完整生成。
/// staging 有 .git + HEAD 时，plan.new_index_sha256 和 ref_plans 应非空。
#[test]
fn prepare_generates_complete_plan_with_staging() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    let _live_repo = git2::Repository::init(&live).unwrap();

    // staging 有 .git + HEAD + 一个 commit。
    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    let staging_repo = git2::Repository::init(&staging).unwrap();
    let mut index = staging_repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    // 显式用 refs/heads/main branch。
    let commit_oid = staging_repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    staging_repo
        .reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
        .unwrap();

    let seed = GitSeedState::Unborn {
        head_ref: "refs/heads/main".to_string(),
    };
    let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging, None).unwrap();
    // plan 应有目标 index hash。
    assert!(
        plan.new_index_sha256.is_some(),
        "plan should have new_index_sha256 when staging has HEAD"
    );
    // plan 应有 ref_plans（创建 refs/heads/main）。
    assert_eq!(plan.ref_plans.len(), 1);
    assert_eq!(plan.ref_plans[0].0, "refs/heads/main");
    assert!(plan.ref_plans[0].1.is_none()); // old_oid = None (unborn)
    assert_eq!(plan.ref_plans[0].2, commit_oid.to_string()); // new_oid
                                                             // snapshot 仍正确。
    assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
    assert!(snapshot.repo_existed);
}

/// #644 评论 5480360027 修复点 3：验证 index lock 边界。
/// 如果 .git/index.lock 已存在，install_index_with_lock 应返回
/// ConcurrentMetadataChanged，不覆盖。
#[test]
fn index_lock_prevents_concurrent_write() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    // staging 有 .git + HEAD。
    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    let staging_repo = git2::Repository::init(&staging).unwrap();
    let mut s_index = staging_repo.index().unwrap();
    s_index.add_path(std::path::Path::new("a.txt")).unwrap();
    s_index.write().unwrap();
    let s_tree_oid = s_index.write_tree().unwrap();
    let s_tree = staging_repo.find_tree(s_tree_oid).unwrap();
    staging_repo
        .commit(Some("HEAD"), &sig, &sig, "init", &s_tree, &[])
        .unwrap();

    // 模拟并发：创建 .git/index.lock。
    let lock_path = live.join(".git").join("index.lock");
    fs::write(&lock_path, b"concurrent").unwrap();

    // install_index_with_lock 应检测到 lock 已存在，返回 ConcurrentMetadataChanged。
    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Symbolic {
            target: "refs/heads/main".to_string(),
        },
        refs: std::collections::BTreeMap::new(),
        index: IndexSnapshot::Missing,
        repo_existed: true,
    };
    let staging_head = staging_repo.head().unwrap();
    let new_oid = staging_head.target().unwrap();
    let result = install_index_with_lock(
        &live,
        &repo,
        &staging_repo,
        new_oid,
        &snapshot,
        "test-owner",
        None,
    );
    assert!(matches!(
        result,
        Err(GitFinalizeError::ConcurrentMetadataChanged { .. })
    ));

    // 清理 lock 文件。
    let _ = fs::remove_file(&lock_path);
}

/// #644 评论 5480360027 修复点 4：验证 ref 反向 CAS rollback。
/// 更新型 ref rollback 用 reference_matching，不 force 覆盖并发修改。
#[test]
fn rollback_ref_uses_reverse_cas() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    // 创建第一个 commit（old_oid）。
    let old_oid = repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    repo.reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
        .unwrap();

    // 创建第二个 commit（new_oid），模拟 finalize 写入。
    fs::write(live.join("b.txt"), "world").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let tree2_oid = index2.write_tree().unwrap();
    let tree2 = repo.find_tree(tree2_oid).unwrap();
    let old_commit = repo.find_commit(old_oid).unwrap();
    let new_oid = repo
        .commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "second",
            &tree2,
            &[&old_commit],
        )
        .unwrap();

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Symbolic {
            target: "refs/heads/main".to_string(),
        },
        refs: std::collections::BTreeMap::from([(
            "refs/heads/main".to_string(),
            RefSnapshot::Existed {
                oid: old_oid.to_string(),
            },
        )]),
        index: IndexSnapshot::Missing,
        repo_existed: true,
    };

    // plan 记录 old_oid -> new_oid。
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
        ref_tx_owner: Some(uuid::Uuid::new_v4().to_string()),
        ref_lock_names: vec!["HEAD".to_string(), "refs/heads/main".to_string()],
    };

    // rollback 应成功（current == new_oid，反向 CAS 回 old_oid）。
    rollback_git_finalize(&live, &snapshot, &plan, None).unwrap();

    // 验证 ref 已恢复到 old_oid（反向 CAS 成功）。
    let repo2 = git2::Repository::open(&live).unwrap();
    let ref_head = repo2.find_reference("refs/heads/main").unwrap();
    assert_eq!(ref_head.target(), Some(old_oid));
}

/// #644 评论 5480360027 修复点 2：验证 plan 在写 live 前完整落盘。
/// prepare_git_finalize 返回的 plan 不依赖 finalize 过程中的内存状态。
#[test]
fn plan_is_complete_before_writing_live() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();

    let staging = tmp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("a.txt"), "hello").unwrap();
    let staging_repo = git2::Repository::init(&staging).unwrap();
    let mut index = staging_repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    // 显式用 refs/heads/main branch。
    let commit_oid = staging_repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    // 设置 HEAD 指向 refs/heads/main。
    staging_repo
        .reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
        .unwrap();

    // NotGitRepo 路径：plan 应完整（repo_create=true, new_index_sha256=Some, ref_plans 非空）。
    let (snapshot, plan) =
        prepare_git_finalize(&live, &GitSeedState::NotGitRepo, &staging, None).unwrap();
    assert!(plan.repo_create);
    assert!(plan.new_index_sha256.is_some());
    assert_eq!(plan.ref_plans.len(), 1);
    assert_eq!(plan.ref_plans[0].0, "refs/heads/main");
    assert!(plan.ref_plans[0].1.is_none()); // NotGitRepo → old=None
    assert_eq!(plan.ref_plans[0].2, commit_oid.to_string());
    // snapshot 是最小快照。
    assert!(!snapshot.repo_existed);
    assert!(matches!(snapshot.index, IndexSnapshot::Missing));
}

/// #644 评论 5483239422 问题2：Git rollback 不幂等。
///
/// 复现策略：构造 live repo，index 已被第一次 rollback 恢复成 snapshot.index（old），
/// plan.new_index_sha256 指向 new_index（不同于当前）。第二次调用
/// `rollback_git_finalize` 应 no-op（current == old → AlreadyReverted）。
/// - 当前行为：`index_is_ours = (current == new)` 为 false，lock 不存在，
///   走 `else` 分支返回 Err（index CAS miss），事务永久卡死。
/// - 预期行为：current == old(snapshot) → no-op，返回 Ok(Reverted)。
///
/// 此测试断言预期行为（第二次 rollback 应成功），当前代码下断言失败。
#[test]
fn rollback_should_be_idempotent_when_index_already_reverted() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    // original index bytes（snapshot.index）
    let original_index = fs::read(live.join(".git").join("index")).unwrap();

    // 修改 index 得到 new_index（plan.new）
    fs::write(live.join("b.txt"), "new").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let new_index = fs::read(live.join(".git").join("index")).unwrap();
    let new_index_hash = sha256_bytes(&new_index);

    // 模拟第一次 rollback 已执行：把 index 恢复成 original（== snapshot.index）。
    fs::write(live.join(".git").join("index"), &original_index).unwrap();

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Symbolic {
            target: "refs/heads/main".to_string(),
        },
        refs: std::collections::BTreeMap::new(),
        index: IndexSnapshot::Bytes(original_index),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(new_index_hash),
        ref_plans: Vec::new(),
        repo_create_owner: None,
        index_lock_owner: None,
        ref_tx_owner: None,
        ref_lock_names: Vec::new(),
    };

    // 第二次 rollback：current index == original (== snapshot.index)。
    // 预期：no-op，返回 Ok(Reverted)。
    // 当前：index CAS miss（current != new），返回 Err。
    let result = rollback_git_finalize(&live, &snapshot, &plan, None);
    assert!(
        result.is_ok(),
        "rollback should be idempotent when index already reverted to snapshot; \
             current code returns Err (index CAS miss because current == old is not \
             recognized as AlreadyReverted), permanently stucking the transaction"
    );
}

/// #644 评论 5483239422 问题2（ref 部分）：ref rollback 不幂等。
///
/// 复现策略：构造 live repo，ref 已被第一次 rollback 反向 CAS 回 old_oid，
/// plan 记录 old_oid -> new_oid。第二次调用 `rollback_git_finalize` 应 no-op
/// （current == old → no-op）。
/// - 当前行为：`old_oid=Some` 时只接受 `current == new_oid`，current == old_oid
///   走 else 分支返回 Err（ref CAS miss），事务永久卡死。
/// - 预期行为：current == old → no-op，返回 Ok(Reverted)。
///
/// 此测试断言预期行为（第二次 rollback 应成功），当前代码下断言失败。
#[test]
fn rollback_should_be_idempotent_when_ref_already_reverted() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    let old_oid = repo
        .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
        .unwrap();
    repo.reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
        .unwrap();

    // 创建第二个 commit（new_oid），模拟 finalize 写入。
    fs::write(live.join("b.txt"), "world").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let tree2_oid = index2.write_tree().unwrap();
    let tree2 = repo.find_tree(tree2_oid).unwrap();
    let old_commit = repo.find_commit(old_oid).unwrap();
    let new_oid = repo
        .commit(
            Some("refs/heads/main"),
            &sig,
            &sig,
            "second",
            &tree2,
            &[&old_commit],
        )
        .unwrap();

    // 模拟第一次 rollback 已执行：把 ref 反向 CAS 回 old_oid。
    repo.reference_matching(
        "refs/heads/main",
        old_oid,
        true,
        new_oid,
        "simulate first rollback",
    )
    .unwrap();

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Symbolic {
            target: "refs/heads/main".to_string(),
        },
        refs: std::collections::BTreeMap::from([(
            "refs/heads/main".to_string(),
            RefSnapshot::Existed {
                oid: old_oid.to_string(),
            },
        )]),
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
        ref_tx_owner: Some(uuid::Uuid::new_v4().to_string()),
        ref_lock_names: vec!["HEAD".to_string(), "refs/heads/main".to_string()],
    };

    // 第二次 rollback：current ref == old_oid (== snapshot ref)。
    // 预期：no-op，返回 Ok(Reverted)。
    // 当前：ref CAS miss（current == old != new），返回 Err。
    let result = rollback_git_finalize(&live, &snapshot, &plan, None);
    assert!(
        result.is_ok(),
        "rollback should be idempotent when ref already reverted to old_oid; \
             current code returns Err (ref CAS miss because current == old is not \
             recognized as no-op), permanently stucking the transaction"
    );
}

/// #644 评论 5483239422 问题3：rollback index 误删外部 Git 进程的 index.lock。
///
/// 复现策略：构造 live index == plan.new（本轮 install 成功），另一个正常 Git
/// 进程创建自己的 `.git/index.lock`。调用 `rollback_git_finalize`：
/// - 当前行为：`index_is_ours = true`，恢复 snapshot.index，然后
///   `if lock_path.exists() { let _ = fs::remove_file(&lock_path); }` 删 lock，
///   破坏外部 git add/checkout/merge。
/// - 预期行为：不应删别人的 lock。应走真正的 lockfile 反向提交边界：用
///   `create_new(true)` 自己获取锁，lock 已存在则返回 ConcurrentChanged。
///
/// 此测试断言预期行为（lock 应仍存在），当前代码下断言失败。
#[test]
fn rollback_should_not_delete_external_index_lock() {
    crate::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = TempDir::new().unwrap();
    let live = tmp.path().join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.txt"), "hello").unwrap();

    let repo = git2::Repository::init(&live).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("a.txt")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();

    // original index bytes（snapshot.index）
    let original_index = fs::read(live.join(".git").join("index")).unwrap();

    // 修改 index 得到 new_index（模拟本轮 install 成功，index == plan.new）
    fs::write(live.join("b.txt"), "new").unwrap();
    let mut index2 = repo.index().unwrap();
    index2.add_path(std::path::Path::new("b.txt")).unwrap();
    index2.write().unwrap();
    let new_index = fs::read(live.join(".git").join("index")).unwrap();
    let new_index_hash = sha256_bytes(&new_index);

    // 另一个正常 Git 进程创建自己的 index.lock（不属于本轮）。
    let lock_path = live.join(".git").join("index.lock");
    fs::write(&lock_path, b"external-git-process-lock-marker").unwrap();

    let snapshot = GitMetadataSnapshot {
        head: RefSnapshot::Symbolic {
            target: "refs/heads/main".to_string(),
        },
        refs: std::collections::BTreeMap::new(),
        index: IndexSnapshot::Bytes(original_index),
        repo_existed: true,
    };
    let plan = GitFinalizePlan {
        repo_create: false,
        new_index_sha256: Some(new_index_hash),
        ref_plans: Vec::new(),
        repo_create_owner: None,
        // #644 评论 5484539222 缺陷1：反向恢复路径需要 owner 做 OwnedIndexLock。
        // acquire 时检测到外部 lock 已存在 → ConcurrentMetadataChanged，不删外部 lock。
        index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
        ref_tx_owner: None,
        ref_lock_names: Vec::new(),
    };

    // rollback：current index == new_index，index_is_ours = true。
    // 当前：恢复 original，然后删 lock（危险）。
    // 预期：不应删别人的 lock。
    let _ = rollback_git_finalize(&live, &snapshot, &plan, None);

    assert!(
        lock_path.exists(),
        "rollback must NOT delete index.lock belonging to another Git process; \
             current code removes it via `let _ = fs::remove_file(&lock_path)` after \
             restoring index, breaking external git add/checkout/merge"
    );
}
